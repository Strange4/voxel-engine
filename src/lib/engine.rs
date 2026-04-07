use std::f32;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use egui_winit_vulkano::{Gui, GuiConfig};
use glam::{Vec3, vec3};
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BlitImageInfo, CommandBufferExecFuture, CommandBufferUsage,
    CopyBufferToImageInfo, PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::physical::PhysicalDevice;
use vulkano::device::{Device, Queue};
use vulkano::format::Format;
use vulkano::image::view::{ImageView, ImageViewCreateInfo};
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, FreeListAllocator, GenericMemoryAllocator, MemoryTypeFilter,
    StandardMemoryAllocator,
};
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
use vulkano::pipeline::{
    ComputePipeline, Pipeline, PipelineBindPoint, PipelineLayout, PipelineShaderStageCreateInfo,
};
use vulkano::query::{QueryPool, QueryPoolCreateInfo, QueryResultFlags, QueryType};
use vulkano::shader::ShaderModule;
use vulkano::swapchain::{
    self, Surface, Swapchain, SwapchainAcquireFuture, SwapchainCreateInfo, SwapchainPresentInfo,
};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::{Validated, VulkanError};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use vulkano::sync::{self, GpuFuture, PipelineStage};

use crate::camera::Camera;
use crate::voxel_data::XYZIVoxelData;
use crate::voxel_loader::VoxFile;
use crate::vulkan::starter::{
    get_device_and_queue, get_headless_device_and_queue, get_headless_instance,
    get_physical_device_and_family_index, get_physical_device_and_family_index_for_surface,
    get_swapchain, get_windowed_instance,
};

type SwapchainFenceFuture = FenceSignalFuture<swapchain::PresentFuture<Box<dyn GpuFuture>>>;
type FenceFuture = FenceSignalFuture<CommandBufferExecFuture<Box<dyn GpuFuture>>>;

const MAX_TIMESTAMP_QUERIES_PER_IMAGE: u32 = 3;

pub struct Engine<T> {
    engine_parts: EngineParts,
    renderer: T,
}

pub struct WindowedEngine {
    // for keeping timestamps in case they aren't available yet
    compute_time: f64,
    copy_time: f64,

    // swapchchain necessities
    recreate_swapchain: bool,
    swapchain: Arc<Swapchain>,
    previous_fence: usize,
    fences: Vec<Option<SwapchainFenceFuture>>,
    should_record: Vec<bool>,

    // for setting which image the engine should render into
    output_images: Vec<Arc<Image>>,
    present_images_and_views: Vec<(Arc<Image>, Arc<ImageView>)>,
    descriptor_sets: Vec<Arc<DescriptorSet>>,

    gui: Gui,
}

pub struct EngineParts {
    camera: Camera,

    // stuff that we want to precompute
    push_contants: PushConstants,
    last_image_size: [u32; 2],

    // Vulkan nececities
    device: Arc<Device>,
    queue: Arc<Queue>,

    // stuff required for the compute shader
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    compute_pipeline: Arc<ComputePipeline>,

    // for timings
    query_pool: Arc<QueryPool>,
    timestamp_period: f64,
}

pub struct HeadlessEngine {
    descriptor_set: Arc<DescriptorSet>,
    fence: Option<FenceFuture>,
    output_image: Arc<Image>,
}

// Vec3's get padded to vec 4's anyway. So I instead of leaving those bytes to waste we use them to represent the camera

// we have to use vec4's instead of vec3's because of how the padding in the std140 works.
// They padd the vec3's to vec4's and when we "read" the vec3's in the shader side they will only read what they need
// please see: https://learnopengl.com/Advanced-OpenGL/Advanced-GLSL Uniform block layout
// and: https://doc.rust-lang.org/reference/type-layout.html#r-layout.repr.align-packed
// cool visualizer: https://maraneshi.github.io/HLSL-ConstantBufferLayoutVisualizer/
#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct PushConstants {
    top_left_pixel: Vec3,
    camera_x: f32,
    pixel_delta_right: Vec3,
    camera_y: f32,
    pixel_delta_down: Vec3,
    camera_z: f32,
}

impl<T> Engine<T> {
    pub fn set_camera(&mut self, camera: Camera) {
        self.engine_parts.camera = camera;
        self.engine_parts.push_contants = compute_ray_dependencies(
            &self.engine_parts.last_image_size,
            &self.engine_parts.camera,
        );
    }
}

impl Engine<WindowedEngine> {
    pub fn new(window: Arc<Window>, event_loop: &ActiveEventLoop) -> Self {
        let (renderer, engine_parts) = WindowedEngine::new_with_parts(window, event_loop);

        Self {
            engine_parts,
            renderer,
        }
    }

    pub fn draw(&mut self, window: &Arc<Window>, window_resized: bool) {
        self.renderer
            .draw::<fn(&mut Gui)>(window, window_resized, &mut self.engine_parts, None);
    }

    pub fn draw_with_gui<RenderGuiFn>(
        &mut self,
        window: &Arc<Window>,
        window_resized: bool,
        gui_draw_fn: RenderGuiFn,
    ) where
        RenderGuiFn: FnOnce(&mut Gui),
    {
        self.renderer.draw(
            window,
            window_resized,
            &mut self.engine_parts,
            Some(gui_draw_fn),
        );
    }

    /// See docs for pass_event_to_gui
    pub fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.renderer.pass_event_to_gui(event)
    }

    pub fn image_draw_time_ns(&self) -> f64 {
        self.renderer.compute_time + self.renderer.copy_time
    }
}

impl Engine<HeadlessEngine> {
    pub fn new(output_image_size: [u32; 2]) -> Self {
        let (renderer, engine_parts) = HeadlessEngine::new_with_parts(output_image_size);
        Self {
            engine_parts,
            renderer,
        }
    }

    pub fn draw(&mut self) -> Option<f64> {
        self.renderer.draw(&self.engine_parts)
    }
}

impl HeadlessEngine {
    fn new_with_parts(output_image_size: [u32; 2]) -> (Self, EngineParts) {
        let instance = get_headless_instance();

        let (physical_device, queue_family_index) = get_physical_device_and_family_index(&instance);
        let (device, queue) =
            get_headless_device_and_queue(physical_device.clone(), queue_family_index);

        let engine_parts = create_engine_parts(
            output_image_size,
            &physical_device,
            device,
            queue,
            MAX_TIMESTAMP_QUERIES_PER_IMAGE,
        );

        let (descriptor_set, output_image) = create_descriptor_set_and_output_image(
            &engine_parts.device,
            &engine_parts.queue,
            &engine_parts.compute_pipeline,
            &output_image_size,
            engine_parts.command_buffer_allocator.clone(),
        );

        let me = Self {
            descriptor_set,
            fence: None,
            output_image,
        };

        (me, engine_parts)
    }

    /// Draws a single image and returns the amount of time that the GPU took to render that image
    ///
    /// Note: this function *waits* for the timings from the GPU to be returned.
    ///
    /// Because of the differences between a windowed engine, comparing benchmarks
    /// should only be done with other benchmarks of this function.
    fn draw(&mut self, engine_parts: &EngineParts) -> Option<f64> {
        let previous_future = match self.fence.take() {
            None => {
                let mut now = sync::now(engine_parts.device.clone());
                now.cleanup_finished();
                now.boxed()
            }
            Some(fence) => {
                fence.wait(None).unwrap();
                fence.boxed()
            }
        };

        let command_buffer = create_draw_to_image_command_buffer(
            engine_parts.push_contants,
            self.descriptor_set.clone(),
            engine_parts.command_buffer_allocator.clone(),
            &self.output_image,
            &engine_parts.queue,
            engine_parts.compute_pipeline.clone(),
            &engine_parts.query_pool,
            true,
            0,
        )
        .build()
        .unwrap();

        let execution = previous_future
            .then_execute(engine_parts.queue.clone(), command_buffer)
            .unwrap()
            .then_signal_fence_and_flush();

        let future = match execution.map_err(Validated::unwrap) {
            Ok(future) => Some(future),
            Err(err) => panic!("{err}"),
        };
        self.fence = future;

        let timings = get_query_timings(
            &engine_parts.query_pool,
            0,
            2,
            engine_parts.timestamp_period,
            true,
        )
        .unwrap();

        let time = timings[1] - timings[0];
        if timings[0] == 0.0 {
            return None;
        }

        Some(time)
    }
}

impl WindowedEngine {
    fn new_with_parts(window: Arc<Window>, event_loop: &ActiveEventLoop) -> (Self, EngineParts) {
        let instance = get_windowed_instance(&window);
        let surface = Surface::from_window(instance.clone(), window.clone()).unwrap();
        let dimensions = window.inner_size();

        let (physical_device, queue_family_index) =
            get_physical_device_and_family_index_for_surface(&surface, &instance);
        let (device, queue) = get_device_and_queue(physical_device.clone(), queue_family_index);

        let (swapchain, images) = get_swapchain(
            device.clone(),
            &physical_device,
            surface.clone(),
            dimensions,
        );

        let extent: [u32; 3] = images[0].extent();
        let last_image_size = [extent[0], extent[1]];

        let compute_shader = cs::load(device.clone()).unwrap();
        let compute_pipeline = get_compute_pipeline(&device, &compute_shader);
        let (output_images, descriptor_sets) =
            create_descriptor_sets_and_output_images(&images, &compute_pipeline, &queue, &device);

        let engine_parts = create_engine_parts(
            last_image_size,
            &physical_device,
            device,
            queue.clone(),
            images.len() as u32 * MAX_TIMESTAMP_QUERIES_PER_IMAGE,
        );

        let images_and_views = get_images_and_views(images);

        let gui = Gui::new(
            event_loop,
            surface,
            queue.clone(),
            images_and_views[0].0.format(), // give the same format as the swapchain format
            GuiConfig {
                is_overlay: true,
                allow_srgb_render_target: true,
                ..Default::default()
            },
        );

        let mut fences = Vec::with_capacity(images_and_views.len());

        // rust can't clone an option of none...
        for _ in 0..images_and_views.len() {
            fences.push(None);
        }

        let renderer = Self {
            compute_time: 0.0,
            copy_time: 0.0,
            recreate_swapchain: false,
            swapchain,
            previous_fence: 0,
            fences,
            should_record: vec![true; images_and_views.len()],
            output_images,
            present_images_and_views: images_and_views,
            descriptor_sets,
            gui: gui,
        };

        (renderer, engine_parts)
    }

    fn draw<RenderFn>(
        &mut self,
        window: &Arc<Window>,
        window_resized: bool,
        engine_parts: &mut EngineParts,
        gui_draw_fn: Option<RenderFn>,
    ) where
        RenderFn: FnOnce(&mut Gui),
    {
        self.handle_recreate_swapchain(window, window_resized, engine_parts);

        // draw the gui before we have to wait for the fence. We will have to wait for the fence less
        if let Some(render_fn) = gui_draw_fn {
            self.gui.immediate_ui(render_fn);
        }

        let maybe_swapchain = self.acquire_next_swapchain_image();
        if maybe_swapchain.is_none() {
            return;
        }

        let (swap_image_index, acquire_future) = maybe_swapchain.unwrap();

        // we have to wait because that image's descriptor set is sill in use.
        // we can't create a new command buffer while it's still in use
        if let Some(image_fence) = &self.fences[swap_image_index as usize] {
            image_fence.wait(None).unwrap();
        }

        let previous_future = match self.fences.remove(self.previous_fence) {
            None => {
                let mut now = sync::now(engine_parts.device.clone());
                now.cleanup_finished();
                now.boxed()
            }
            Some(fence) => fence.boxed(),
        };

        let command_buffer = create_draw_to_swapchain_command_buffer(
            engine_parts.push_contants,
            self.descriptor_sets[swap_image_index as usize].clone(),
            engine_parts.command_buffer_allocator.clone(),
            self.present_images_and_views[swap_image_index as usize]
                .0
                .clone(),
            self.output_images[swap_image_index as usize].clone(),
            &engine_parts.queue,
            engine_parts.compute_pipeline.clone(),
            engine_parts.query_pool.clone(),
            swap_image_index,
            self.should_record[swap_image_index as usize],
        );

        self.update_query_timings(
            swap_image_index,
            &engine_parts.query_pool,
            engine_parts.timestamp_period,
        );

        let execution = previous_future
            .join(acquire_future)
            .then_execute(engine_parts.queue.clone(), command_buffer)
            .unwrap();

        let execution = self
            .gui
            .draw_on_image(
                execution,
                self.present_images_and_views[swap_image_index as usize]
                    .1
                    .clone(),
            )
            .then_swapchain_present(
                engine_parts.queue.clone(),
                SwapchainPresentInfo::swapchain_image_index(
                    self.swapchain.clone(),
                    swap_image_index,
                ),
            )
            .then_signal_fence_and_flush();

        let future = match execution.map_err(Validated::unwrap) {
            Ok(future) => Some(future),
            Err(VulkanError::OutOfDate) => {
                self.recreate_swapchain = true;
                None
            }
            Err(err) => panic!("{err}"),
        };
        self.fences.insert(swap_image_index as usize, future);
        // self.fences[swap_image_index as usize] = future;

        self.previous_fence = swap_image_index as usize;
    }

    /// Returns true when the event should NOT be passed to the rest of the renderer. False when it should
    ///
    /// e.g. when you click on a egui window you don't want it to go to the renderer
    fn pass_event_to_gui(&mut self, event: &WindowEvent) -> bool {
        self.gui.update(event)
    }

    fn update_query_timings(
        &mut self,
        swap_image_index: u32,
        query_pool: &Arc<QueryPool>,
        timestamp_period: f64,
    ) {
        let timestamp_index = swap_image_index * MAX_TIMESTAMP_QUERIES_PER_IMAGE;
        let query_timings = get_query_timings(
            query_pool,
            timestamp_index,
            MAX_TIMESTAMP_QUERIES_PER_IMAGE,
            timestamp_period,
            false,
        );
        if let Some(timings) = &query_timings {
            self.compute_time = timings[1] - timings[0];
            self.copy_time = timings[2] - timings[1];
        }

        self.should_record[swap_image_index as usize] = query_timings.is_some();
    }

    fn handle_recreate_swapchain(
        &mut self,
        window: &Arc<Window>,
        window_resized: bool,
        engine_parts: &mut EngineParts,
    ) {
        if !self.recreate_swapchain && !window_resized {
            return;
        }

        let new_dimensions = window.inner_size();
        self.recreate_swapchain = false;
        let (new_swapchain, new_images) = self
            .swapchain
            .recreate(SwapchainCreateInfo {
                image_extent: new_dimensions.into(),
                ..self.swapchain.create_info()
            })
            .unwrap();
        self.swapchain = new_swapchain;

        if !window_resized {
            return;
        }

        let (output_images, descriptor_sets) = create_descriptor_sets_and_output_images(
            &new_images,
            &engine_parts.compute_pipeline,
            &engine_parts.queue,
            &engine_parts.device,
        );
        self.present_images_and_views = get_images_and_views(new_images);
        self.output_images = output_images;
        self.descriptor_sets = descriptor_sets;
        engine_parts.last_image_size = [new_dimensions.width, new_dimensions.height];

        engine_parts.push_contants =
            compute_ray_dependencies(&engine_parts.last_image_size, &engine_parts.camera);
    }

    fn acquire_next_swapchain_image(&mut self) -> Option<(u32, SwapchainAcquireFuture)> {
        let err =
            swapchain::acquire_next_image(self.swapchain.clone(), None).map_err(Validated::unwrap);

        let (swap_image_index, suboptimal_image, acquire_future) = match err {
            Ok(r) => r,
            Err(VulkanError::OutOfDate) => {
                self.recreate_swapchain = true;
                return None;
            }
            Err(err) => panic!("{}", err),
        };

        if suboptimal_image {
            self.recreate_swapchain = true;
        }

        Some((swap_image_index, acquire_future))
    }
}

/// please input the TOTAL number_of_queries that the query pool can have
/// If you have a swapchain and each image does a query, multiply them
fn create_engine_parts(
    output_image_size: [u32; 2],
    physical_device: &Arc<PhysicalDevice>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    query_count: u32,
) -> EngineParts {
    let camera = Camera::default();
    let push_contants = compute_ray_dependencies(&output_image_size, &camera);

    let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));
    let compute_shader = cs::load(device.clone()).unwrap();
    let compute_pipeline = get_compute_pipeline(&device, &compute_shader);

    let query_pool = QueryPool::new(
        device.clone(),
        QueryPoolCreateInfo {
            query_count,
            ..QueryPoolCreateInfo::query_type(QueryType::Timestamp)
        },
    )
    .unwrap();

    EngineParts {
        camera,
        push_contants,
        device,
        queue,
        command_buffer_allocator,
        compute_pipeline,
        query_pool,
        timestamp_period: physical_device.properties().timestamp_period as f64,
        last_image_size: output_image_size,
    }
}

fn get_query_timings(
    query_pool: &Arc<QueryPool>,
    query_index_start: u32,
    number_of_queries: u32,
    timestamp_period: f64,
    wait_for_query: bool,
) -> Option<Vec<f64>> {
    if wait_for_query {
        let mut timing_results: Vec<u64> = vec![0; number_of_queries as usize];
        query_pool
            .get_results(
                query_index_start..query_index_start + number_of_queries,
                &mut timing_results,
                QueryResultFlags::WAIT,
            )
            .unwrap();
        let results = timing_results
            .into_iter()
            .map(|value| value as f64 * timestamp_period)
            .collect();
        return Some(results);
    }

    let mut timing_results: Vec<u64> = vec![0; (number_of_queries * 2) as usize];
    query_pool
        .get_results(
            query_index_start..query_index_start + number_of_queries,
            &mut timing_results,
            QueryResultFlags::WITH_AVAILABILITY,
        )
        .unwrap();
    let all_available = !timing_results
        .iter()
        .enumerate()
        .any(|(index, &value)| index % 2 == 1 && value == 0);

    if !all_available {
        return None;
    }
    let results = timing_results
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if index % 2 == 1 {
                None
            } else {
                Some(value as f64 * timestamp_period)
            }
        })
        .collect();
    Some(results)
}

fn compute_ray_dependencies(image_size: &[u32; 2], camera: &Camera) -> PushConstants {
    let up_vector: Vec3 = camera.up;
    let image_width = image_size[0] as f32;
    let image_height = image_size[1] as f32;
    let aspect_ratio = image_width / image_height;

    let camera_center = camera.position;

    let viewport_height =
        2.0 * camera.focal_distance * (camera.field_of_view.to_radians() / 2.0).tan();
    let viewport_width = aspect_ratio * viewport_height;
    let camera_relative_forward = camera.direction.normalize();

    let camera_relative_right = camera_relative_forward.cross(up_vector).normalize();
    let camera_relative_down = camera_relative_forward.cross(camera_relative_right);

    let viewport_right_vector = viewport_width * camera_relative_right;
    let viewport_down_vector = viewport_height * camera_relative_down;

    let pixel_delta_right = viewport_right_vector / image_width;
    let pixel_delta_down = viewport_down_vector / image_height;

    let viepwort_upper_left = 0.5 * (-viewport_down_vector - viewport_right_vector)
        + camera_center
        + camera_relative_forward * camera.focal_distance;

    let top_left_pixel = viepwort_upper_left + 0.5 * (pixel_delta_down + pixel_delta_right);

    PushConstants {
        top_left_pixel,
        pixel_delta_right,
        pixel_delta_down,
        camera_x: camera_center.x,
        camera_y: camera_center.y,
        camera_z: camera_center.z,
    }
}

fn get_images_and_views(images: Vec<Arc<Image>>) -> Vec<(Arc<Image>, Arc<ImageView>)> {
    images
        .into_iter()
        .map(|image| {
            let view = ImageView::new(
                image.clone(),
                ImageViewCreateInfo {
                    format: image.format(),
                    subresource_range: image.subresource_range(),
                    ..Default::default()
                },
            )
            .unwrap();
            (image, view)
        })
        .collect()
}

fn get_compute_pipeline(
    device: &Arc<Device>,
    compute_shader: &Arc<ShaderModule>,
) -> Arc<ComputePipeline> {
    let cs = compute_shader.entry_point("main").unwrap();
    let stage = PipelineShaderStageCreateInfo::new(cs);
    let layout = PipelineLayout::new(
        device.clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages([&stage])
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .unwrap();
    ComputePipeline::new(
        device.clone(),
        None,
        ComputePipelineCreateInfo::stage_layout(stage, layout),
    )
    .unwrap()
}

// Gets the command buffer for a single dispatch of the compute shader.
// This is done so that we can modify the push constants every frame.
fn create_draw_to_swapchain_command_buffer(
    push_constants: PushConstants,
    descriptor_set: Arc<DescriptorSet>,
    allocator: Arc<StandardCommandBufferAllocator>,
    present_image: Arc<Image>,
    output_image: Arc<Image>,
    queue: &Arc<Queue>,
    pipeline: Arc<ComputePipeline>,
    query_pool: Arc<QueryPool>,
    swapchain_index: u32,
    should_write_timestamp: bool,
) -> Arc<PrimaryAutoCommandBuffer> {
    let timestamp_index = swapchain_index * MAX_TIMESTAMP_QUERIES_PER_IMAGE;

    let mut builder = create_draw_to_image_command_buffer(
        push_constants,
        descriptor_set,
        allocator,
        &output_image,
        queue,
        pipeline,
        &query_pool,
        should_write_timestamp,
        timestamp_index,
    );
    builder
        .blit_image(BlitImageInfo::images(output_image, present_image))
        .unwrap();

    unsafe {
        // the query pool was reset in the previous function
        builder
            .write_timestamp(
                query_pool.clone(),
                timestamp_index + 2,
                PipelineStage::TopOfPipe,
            )
            .unwrap();
    }

    builder.build().unwrap()
}

/// Creates a new command buffer that:
/// - resets the queries and creates 2 new timings if `should_write_timestamp` is true
/// - dispatches the shader
fn create_draw_to_image_command_buffer(
    push_constants: PushConstants,
    descriptor_set: Arc<DescriptorSet>,
    allocator: Arc<StandardCommandBufferAllocator>,
    output_image: &Arc<Image>,
    queue: &Arc<Queue>,
    pipeline: Arc<ComputePipeline>,
    query_pool: &Arc<QueryPool>,
    should_write_timestamp: bool,
    timestamp_index: u32,
) -> AutoCommandBufferBuilder<PrimaryAutoCommandBuffer> {
    let mut builder = AutoCommandBufferBuilder::primary(
        allocator,
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .unwrap();
    let pipeline_layout = pipeline.layout();

    builder
        .bind_pipeline_compute(pipeline.clone())
        .unwrap()
        .push_constants(pipeline_layout.clone(), 0, push_constants)
        .unwrap()
        .bind_descriptor_sets(
            PipelineBindPoint::Compute,
            pipeline_layout.clone(),
            0,
            descriptor_set,
        )
        .unwrap();

    if should_write_timestamp {
        // safety: this the queries are not used in any other command buffer since there is no other command buffer
        unsafe {
            builder
                .reset_query_pool(
                    query_pool.clone(),
                    timestamp_index..timestamp_index + MAX_TIMESTAMP_QUERIES_PER_IMAGE,
                )
                .unwrap()
                .write_timestamp(
                    query_pool.clone(),
                    timestamp_index,
                    PipelineStage::TopOfPipe,
                )
                .unwrap();
        }
    }

    let extent = output_image.extent();
    let local_size_in_shader = 16;

    // The safety requirements are verifiable since only one descriptor set is given
    unsafe {
        builder
            .dispatch([
                extent[0].div_ceil(local_size_in_shader),
                extent[1].div_ceil(local_size_in_shader),
                1,
            ])
            .unwrap();
    }

    unsafe {
        builder
            .write_timestamp(
                query_pool.clone(),
                timestamp_index + 1,
                PipelineStage::BottomOfPipe,
            )
            .unwrap();
    }

    builder
}

fn create_descriptor_set_and_output_image(
    device: &Arc<Device>,
    queue: &Arc<Queue>,
    pipeline: &Arc<ComputePipeline>,
    image_size: &[u32; 2],
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
) -> (Arc<DescriptorSet>, Arc<Image>) {
    let pipeline_layout = pipeline.layout();
    let descriptor_set_layout = pipeline_layout.set_layouts().first().unwrap();
    let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
        device.clone(),
        Default::default(),
    ));

    let allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));

    let model = create_model_and_fill(
        device.clone(),
        allocator.clone(),
        command_buffer_allocator,
        queue.clone(),
    );

    let model_image_view = ImageView::new_default(model).unwrap();
    get_descriptor_set_and_output_image(
        [image_size[0], image_size[1], 1],
        model_image_view,
        allocator,
        descriptor_set_allocator,
        descriptor_set_layout.clone(),
    )
}

fn get_descriptor_set_and_output_image(
    image_extent: [u32; 3],
    model_image_view: Arc<ImageView>,
    allocator: Arc<StandardMemoryAllocator>,
    descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    descriptor_set_layout: Arc<DescriptorSetLayout>,
) -> (Arc<DescriptorSet>, Arc<Image>) {
    let output_image = Image::new(
        allocator,
        ImageCreateInfo {
            format: Format::R8G8B8A8_UNORM,
            extent: image_extent,
            image_type: ImageType::Dim2d,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .unwrap();

    let output_image_view = ImageView::new_default(output_image.clone()).unwrap();
    let descriptor_set = DescriptorSet::new(
        descriptor_set_allocator,
        descriptor_set_layout,
        [
            WriteDescriptorSet::image_view(0, output_image_view.clone()),
            WriteDescriptorSet::image_view(1, model_image_view.clone()),
        ],
        [],
    )
    .unwrap();

    (descriptor_set, output_image)
}
fn create_descriptor_sets_and_output_images(
    present_images: &[Arc<Image>],
    pipeline: &Arc<ComputePipeline>,
    queue: &Arc<Queue>,
    device: &Arc<Device>,
) -> (Vec<Arc<Image>>, Vec<Arc<DescriptorSet>>) {
    let pipeline_layout = pipeline.layout();
    let descriptor_set_layout = pipeline_layout.set_layouts().first().unwrap();
    let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
        device.clone(),
        Default::default(),
    ));
    let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));

    let allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));

    let model = create_model_and_fill(
        device.clone(),
        allocator.clone(),
        command_buffer_allocator.clone(),
        queue.clone(),
    );

    let model_image_view = ImageView::new_default(model).unwrap();

    present_images
        .iter()
        .map(|present_image| {
            let a = get_descriptor_set_and_output_image(
                present_image.extent(),
                model_image_view.clone(),
                allocator.clone(),
                descriptor_set_allocator.clone(),
                descriptor_set_layout.clone(),
            );
            (a.1, a.0)
        })
        .unzip()
}

fn create_model_and_fill(
    device: Arc<Device>,
    allocator: Arc<GenericMemoryAllocator<FreeListAllocator>>,
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    queue: Arc<Queue>,
) -> Arc<Image> {
    let vox_file = VoxFile::load_vox_file(Path::new("models/monu9.vox")).unwrap();
    let voxel_data = XYZIVoxelData::from_vox_file(vox_file).unwrap();
    let size = voxel_data.size();
    let extent = [size.x, size.y, size.z];
    let image = Image::new(
        allocator.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim3d,
            format: Format::R8G8B8A8_UNORM,
            extent,
            usage: ImageUsage::SAMPLED | ImageUsage::STORAGE | ImageUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .unwrap();

    let buffer = Buffer::from_iter(
        allocator.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_SRC | BufferUsage::STORAGE_BUFFER,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        },
        voxel_data.as_rgba_bytes(),
    )
    .expect("Couldn't create buffer");

    let mut builder = AutoCommandBufferBuilder::primary(
        command_buffer_allocator,
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .unwrap();

    builder
        .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(buffer, image.clone()))
        .unwrap();

    let command_buffer = builder.build().unwrap();

    sync::now(device.clone())
        .then_execute(queue.clone(), command_buffer)
        .unwrap()
        .then_signal_fence_and_flush()
        .unwrap()
        .wait(Some(Duration::from_secs(3)))
        .unwrap();
    image
}

fn get_sphere_model(cube_side_length: u32) -> Vec<u8> {
    let diameter = cube_side_length as usize;
    let bytes_per_voxel = 4;
    let mut data = vec![0; diameter * diameter * diameter * bytes_per_voxel];
    let radius = diameter / 2;
    for z in 0..diameter {
        for y in 0..diameter {
            for x in 0..diameter {
                let x_dist = radius.abs_diff(x);
                let y_dist = radius.abs_diff(y);
                let z_dist = radius.abs_diff(z);
                if (x_dist * x_dist + y_dist * y_dist + z_dist * z_dist) <= (radius * radius) {
                    let begin = z * diameter * diameter * bytes_per_voxel
                        + y * diameter * bytes_per_voxel
                        + x * bytes_per_voxel;
                    let color = ((x + y + z) % 2) * 0xb8bb26;

                    data[begin] = ((color & 0xFF0000) >> 16) as u8; // red;
                    data[begin + 1] = ((color & 0x00FF00) >> 8) as u8; // blue;
                    data[begin + 2] = (color & 0x0000FF) as u8; // green;
                    data[begin + 3] = 0xFF; // alpha
                }
            }
        }
    }
    data
}

fn get_bulb_model(cube_side_length: u32) -> Vec<u8> {
    let cube_side_length = cube_side_length as usize;
    let bytes_per_voxel = 4;
    let mut data =
        vec![0; cube_side_length * cube_side_length * cube_side_length * bytes_per_voxel];
    let max_mandel_distance = 1.25;
    for z in 0..cube_side_length {
        for y in 0..cube_side_length {
            for x in 0..cube_side_length {
                let maped_x = (x as f32 / cube_side_length as f32) * max_mandel_distance * 2.0
                    - max_mandel_distance;
                let maped_y = (y as f32 / cube_side_length as f32) * max_mandel_distance * 2.0
                    - max_mandel_distance;
                let mapped_z = (z as f32 / cube_side_length as f32) * max_mandel_distance * 2.0
                    - max_mandel_distance;
                if point_is_part_of_mandelbulb(vec3(maped_x, maped_y, mapped_z)) {
                    let begin = z * cube_side_length * cube_side_length * bytes_per_voxel
                        + y * cube_side_length * bytes_per_voxel
                        + x * bytes_per_voxel;
                    let color = ((x + y + z) % 2) * 0xb8bb26;

                    data[begin] = ((color & 0xFF0000) >> 16) as u8; // red;
                    data[begin + 1] = ((color & 0x00FF00) >> 8) as u8; // green;
                    data[begin + 2] = (color & 0x0000FF) as u8; // blue;
                    data[begin + 3] = 0xFF; // alpha
                }
            }
        }
    }

    data
}

fn point_is_part_of_mandelbulb(point: Vec3) -> bool {
    let mut result = point.clone();

    let max_iters = 4;
    for _ in 0..max_iters {
        // I have no idea how this works
        let (x, y, z) = (result.x, result.y, result.z);
        let (x2, y2, z2) = (x * x, y * y, z * z);
        let (x4, y4, z4) = (x2 * x2, y2 * y2, z2 * z2);

        let k3 = x2 + z2;
        let k2 = 1.0 / (k3 * k3 * k3 * k3 * k3 * k3 * k3).sqrt();
        let k1 = x4 + y4 + z4 - 6.0 * y2 * z2 - 6.0 * x2 * y2 + 2.0 * z2 * x2;
        let k4 = x2 - y2 + z2;
        result.x = 64.0 * x * y * z * (x2 - z2) * k4 * (x4 - 6.0 * x2 * z2 + z4) * k1 * k2;
        result.y = -16.0 * y2 * k3 * k4 * k4 + k1 * k1;
        result.z = -8.0
            * y
            * k4
            * (x4 * x4 - 28.0 * x4 * x2 * z2 + 70.0 * x4 * z4 - 28.0 * x2 * z2 * z4 + z4 * z4)
            * k1
            * k2;

        result += point;
        if result.length_squared() > 256.0 {
            return false;
        }
    }
    true
}

mod cs {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "shaders/main.comp"
    }
}
