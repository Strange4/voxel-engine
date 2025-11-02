use std::f32;
use std::sync::Arc;
use std::time::Duration;

use egui::Align2;
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
use vulkano::query::{self, QueryPool, QueryPoolCreateInfo, QueryResultFlags, QueryType};
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

use crate::vulkan::starter::{
    get_device_and_queue, get_headless_device_and_queue, get_headless_instance,
    get_physical_device_and_family_index, get_physical_device_and_family_index_for_surface,
    get_swapchain, get_windowed_instance,
};

type CommandBufferFence = Arc<FenceSignalFuture<swapchain::PresentFuture<Box<dyn GpuFuture>>>>;

const MAX_TIMESTAMP_QUERIES_PER_IMAGE: u32 = 3;

pub struct Engine {
    // This is here so that we can modify it every frame by the app.
    // using a spherical coordinate system: https://en.m.wikipedia.org/wiki/Spherical_coordinate_system
    camera_x_radians: f32, // the angle off of the x vector
    camera_y_radians: f32, // the angle off of the z vector
    camera_radius: f32,

    // stuff that we want to precompute
    ray_dependencies: PushConstants,
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
    compute_time: f64,
    copy_time: f64,

    // swapchchain necessities
    recreate_swapchain: bool,
    swapchain: Arc<Swapchain>,
    previous_fence: usize,
    fences: Vec<Option<CommandBufferFence>>,
    should_record: Vec<bool>,

    // for setting which image the engine should render into
    output_images: Vec<Arc<Image>>,
    present_images: Vec<(Arc<Image>, Arc<ImageView>)>,
    descriptor_sets: Vec<Arc<DescriptorSet>>,

    gui: Gui,
}

struct EngineParts {
    camera_x_radians: f32, // the angle off of the x vector
    camera_y_radians: f32, // the angle off of the z vector
    camera_radius: f32,

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
    engine_parts: EngineParts,
    descriptor_set: Arc<DescriptorSet>,
    fence: Option<Arc<FenceSignalFuture<CommandBufferExecFuture<Box<dyn GpuFuture>>>>>,
    output_image: Arc<Image>,
}

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

impl HeadlessEngine {
    pub fn new(output_image_size: [u32; 2]) -> Self {
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

        Self {
            engine_parts,
            descriptor_set,
            fence: None,
            output_image,
        }
    }

    /// Draws a single image and returns the amount of time that the GPU took to render that image
    /// 
    /// Note: this function *waits* for the timings from the GPU to be returned.
    /// 
    /// Because of the differences between a windowed engine, comparing benchmarks 
    /// should only be done with other benchmarks of this function.
    pub fn draw(&mut self) -> Option<f64> {
        let previous_future = match self.fence.clone() {
            None => {
                let mut now = sync::now(self.engine_parts.device.clone());
                now.cleanup_finished();
                now.boxed()
            }
            Some(fence) => {
                fence.wait(None).unwrap();
                fence.boxed()
            }
        };

        let command_buffer = create_draw_to_image_command_buffer(
            self.engine_parts.push_contants.clone(),
            self.descriptor_set.clone(),
            self.engine_parts.command_buffer_allocator.clone(),
            &self.output_image,
            &self.engine_parts.queue,
            self.engine_parts.compute_pipeline.clone(),
            &self.engine_parts.query_pool,
            true,
            0,
        )
        .build()
        .unwrap();

        let execution = previous_future
            .then_execute(self.engine_parts.queue.clone(), command_buffer)
            .unwrap()
            .then_signal_fence_and_flush();

        let future = match execution.map_err(Validated::unwrap) {
            Ok(future) => Some(Arc::new(future)),
            Err(err) => panic!("{err}"),
        };
        self.fence = future;

        let timings = get_query_timings(
            &self.engine_parts.query_pool,
            0,
            2,
            self.engine_parts.timestamp_period,
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

impl Engine {
    pub fn new(window: Arc<Window>, event_loop: &ActiveEventLoop) -> Self {
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

        let query_pool = QueryPool::new(
            device.clone(),
            QueryPoolCreateInfo {
                query_count: images.len() as u32 * MAX_TIMESTAMP_QUERIES_PER_IMAGE,
                ..QueryPoolCreateInfo::query_type(QueryType::Timestamp)
            },
        )
        .unwrap();

        let images_and_views = get_images_and_views(images);

        let gui = Gui::new(
            event_loop,
            surface,
            queue.clone(),
            images_and_views[0].0.format(), // give the same format as the swapchain format
            GuiConfig {
                is_overlay: true,
                ..Default::default()
            },
        );

        let camera_y_radians = f32::consts::FRAC_PI_2;
        let camera_x_radians = -f32::consts::FRAC_PI_2;
        let camera_radius = 40.0;

        let ray_dependencies = compute_ray_dependencies(
            &last_image_size,
            camera_y_radians,
            camera_x_radians,
            camera_radius,
        );

        Self {
            recreate_swapchain: false,
            previous_fence: 0,
            device: device.clone(),
            compute_pipeline,
            swapchain,
            queue,
            fences: vec![None; images_and_views.len()],
            descriptor_sets,
            output_images,
            command_buffer_allocator: Arc::new(StandardCommandBufferAllocator::new(
                device.clone(),
                Default::default(),
            )),
            should_record: vec![true; images_and_views.len()],
            present_images: images_and_views,
            camera_x_radians,
            camera_y_radians,
            camera_radius,
            query_pool,
            timestamp_period: physical_device.properties().timestamp_period as f64,
            gui,
            compute_time: 0.0,
            copy_time: 0.0,
            ray_dependencies,
            last_image_size,
        }
    }

    pub fn draw(&mut self, window: &Arc<Window>, window_resized: bool) {
        self.handle_recreate_swapchain(window, window_resized);

        // draw the gui before we have to wait for the fence. We will have to wait for the fence less
        draw_gui(&mut self.gui, self.compute_time, self.copy_time);

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

        let previous_future = match self.fences[self.previous_fence].clone() {
            None => {
                let mut now = sync::now(self.device.clone());
                now.cleanup_finished();
                now.boxed()
            }
            Some(fence) => fence.boxed(),
        };

        let command_buffer = create_draw_to_swapchain_command_buffer(
            self.ray_dependencies,
            self.descriptor_sets[swap_image_index as usize].clone(),
            self.command_buffer_allocator.clone(),
            self.present_images[swap_image_index as usize].0.clone(),
            self.output_images[swap_image_index as usize].clone(),
            &self.queue,
            self.compute_pipeline.clone(),
            self.query_pool.clone(),
            swap_image_index,
            self.should_record[swap_image_index as usize],
        );

        self.update_query_timings(swap_image_index);

        let execution = previous_future
            .join(acquire_future)
            .then_execute(self.queue.clone(), command_buffer)
            .unwrap();

        let execution = self
            .gui
            .draw_on_image(
                execution,
                self.present_images[swap_image_index as usize].1.clone(),
            )
            .then_swapchain_present(
                self.queue.clone(),
                SwapchainPresentInfo::swapchain_image_index(
                    self.swapchain.clone(),
                    swap_image_index,
                ),
            )
            .then_signal_fence_and_flush();

        let future = match execution.map_err(Validated::unwrap) {
            Ok(future) => Some(Arc::new(future)),
            Err(VulkanError::OutOfDate) => {
                self.recreate_swapchain = true;
                None
            }
            Err(err) => panic!("{err}"),
        };
        self.fences[swap_image_index as usize] = future;

        self.previous_fence = swap_image_index as usize;
    }

    /// Returns true when the event should NOT be passed to the rest of the renderer. False when it should
    ///
    /// e.g. when you click on a egui window you don't want it to go to the renderer
    pub fn pass_event_to_gui(&mut self, event: &WindowEvent) -> bool {
        self.gui.update(event)
    }

    pub fn move_horizontally(&mut self, amount_radians: f32) {
        self.camera_x_radians += amount_radians;
        self.ray_dependencies = compute_ray_dependencies(
            &self.last_image_size,
            self.camera_y_radians,
            self.camera_x_radians,
            self.camera_radius,
        );
    }

    pub fn move_vertically(&mut self, amount_radians: f32) {
        self.camera_y_radians =
            (self.camera_y_radians + amount_radians).clamp(0.001, f32::consts::PI - 0.001);
        self.ray_dependencies = compute_ray_dependencies(
            &self.last_image_size,
            self.camera_y_radians,
            self.camera_x_radians,
            self.camera_radius,
        );
    }

    pub fn move_forward(&mut self, amount: f32) {
        self.camera_radius += amount;
        self.ray_dependencies = compute_ray_dependencies(
            &self.last_image_size,
            self.camera_y_radians,
            self.camera_x_radians,
            self.camera_radius,
        );
    }

    fn update_query_timings(&mut self, swap_image_index: u32) {
        let timestamp_index = swap_image_index * MAX_TIMESTAMP_QUERIES_PER_IMAGE;
        let query_timings = get_query_timings(
            &self.query_pool,
            timestamp_index,
            MAX_TIMESTAMP_QUERIES_PER_IMAGE,
            self.timestamp_period,
            false,
        );
        if let Some(timings) = &query_timings {
            self.compute_time = timings[1] - timings[0];
            self.copy_time = timings[2] - timings[1];
        }

        self.should_record[swap_image_index as usize] = query_timings.is_some();
    }

    fn handle_recreate_swapchain(&mut self, window: &Arc<Window>, window_resized: bool) {
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
            &self.compute_pipeline,
            &self.queue,
            &self.device,
        );
        self.present_images = get_images_and_views(new_images);
        self.output_images = output_images;
        self.descriptor_sets = descriptor_sets;
        self.last_image_size = [new_dimensions.width, new_dimensions.height];
        self.ray_dependencies = compute_ray_dependencies(
            &self.last_image_size,
            self.camera_y_radians,
            self.camera_x_radians,
            self.camera_radius,
        );
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

fn compute_ray_dependencies(
    image_size: &[u32; 2],
    camera_y_radians: f32,
    camera_x_radians: f32,
    camera_radius: f32,
) -> PushConstants {
    const VIEWPORT_HEIGHT: f32 = 30.0;
    const UP_VECTOR: Vec3 = vec3(0.0, -1.0, 0.0);
    let image_width = image_size[0] as f32;
    let image_height = image_size[1] as f32;
    let sin = camera_y_radians.sin();
    let x = camera_radius * camera_x_radians.cos() * sin;
    let y = camera_radius * camera_y_radians.cos();
    let z = camera_radius * camera_x_radians.sin() * sin;
    let camera_center = vec3(x, y, z);

    let aspect_ratio = image_width / image_height;
    let viewport_width = aspect_ratio * VIEWPORT_HEIGHT;
    let camera_relative_forward = (-camera_center).normalize();
    let camera_relative_right = camera_relative_forward.cross(UP_VECTOR).normalize();
    let camera_relative_down = camera_relative_forward.cross(camera_relative_right);

    let viewport_right_vector = viewport_width * camera_relative_right;
    let viewport_down_vector = VIEWPORT_HEIGHT * camera_relative_down;

    let pixel_delta_right = viewport_right_vector / image_width;
    let pixel_delta_down = viewport_down_vector / image_height;

    let viepwort_upper_left = -viewport_down_vector * 0.5 - viewport_right_vector * 0.5;
    let top_left_pixel = viepwort_upper_left + 0.5 * (pixel_delta_down + pixel_delta_right);

    return PushConstants {
        top_left_pixel,
        pixel_delta_right,
        pixel_delta_down,
        camera_x: camera_center.x,
        camera_y: camera_center.y,
        camera_z: camera_center.z,
    };
}

fn draw_gui(gui: &mut Gui, compute_time: f64, copy_time: f64) {
    let compute_time_ms = compute_time / 1_000_000.0;
    let copy_time_ms = copy_time / 1_000_000.0;
    gui.immediate_ui(|gui| {
        let ctx = gui.context();

        egui::Window::new("Specs")
            .anchor(Align2::LEFT_TOP, [5.0, 5.0])
            .auto_sized()
            .show(&ctx, |ui| {
                ui.label(format!("Compute time: {compute_time_ms:.3}ns"));
                ui.label(format!("Copy time: {copy_time_ms:.3}ns"));
            });
    });
}

fn get_images_and_views(images: Vec<Arc<Image>>) -> Vec<(Arc<Image>, Arc<ImageView>)> {
    images
        .into_iter()
        .map(|image| {
            (
                image.clone(),
                ImageView::new(
                    image.clone(),
                    ImageViewCreateInfo {
                        format: image.format(),
                        subresource_range: image.subresource_range(),
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
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

/// `number_of_queries` is the TOTAL number_of_queries that the query pool can have
/// If you have a swapchain and each image does a query, multiply them
fn create_engine_parts(
    output_image_size: [u32; 2],
    physical_device: &Arc<PhysicalDevice>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    query_count: u32,
) -> EngineParts {
    let camera_y_radians = f32::consts::FRAC_PI_2;
    let camera_x_radians = -f32::consts::FRAC_PI_2;
    let camera_radius = 40.0;
    let push_contants = compute_ray_dependencies(
        &output_image_size,
        camera_y_radians,
        camera_x_radians,
        camera_radius,
    );

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
        camera_x_radians,
        camera_y_radians,
        camera_radius,
        push_contants,
        last_image_size: output_image_size,
        device,
        queue,
        command_buffer_allocator,
        compute_pipeline,
        query_pool,
        timestamp_period: physical_device.properties().timestamp_period as f64,
    }
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
    let diameter = 10;
    let extent = [diameter; 3];
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
        get_model_data(diameter),
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
fn get_model_data(diameter: u32) -> Vec<u8> {
    let bytes_per_texel = 4;
    let diameter = diameter as usize;
    let mut data = vec![0; (diameter * bytes_per_texel * diameter * diameter) as usize];
    let radius = diameter / 2;
    for z in 0..diameter {
        for y in 0..diameter {
            for x in 0..diameter {
                let x_dist = radius.abs_diff(x);
                let y_dist = radius.abs_diff(y);
                let z_dist = radius.abs_diff(z);
                if (x_dist * x_dist + y_dist * y_dist + z_dist * z_dist) <= (radius * radius) {
                    let begin = z * diameter * diameter * bytes_per_texel
                        + y * diameter * bytes_per_texel
                        + x * bytes_per_texel;
                    let color = ((x + y + z) % 2) * 0xb8bb26;
                    data[begin] = ((color & 0xFF0000) >> 16) as u8; // red;
                    data[begin + 1] = ((color & 0x00FF00) >> 8) as u8; // red;
                    data[begin + 2] = (color & 0x0000FF) as u8; // red;
                    data[begin + 3] = 0xFF; // alpha
                }
            }
        }
    }
    data
}

mod cs {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "shaders/main.comp"
    }
}
