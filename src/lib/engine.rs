mod compute_shader;
mod engine_parts;
mod push_constants;

use egui_winit_vulkano::{Gui, GuiConfig};
use glam::{Vec3, vec3};
use std::path::Path;
use std::sync::Arc;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BlitImageInfo, CommandBufferExecFuture, CommandBufferUsage,
    PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::format::Format;
use vulkano::image::view::{ImageView, ImageViewCreateInfo};
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::pipeline::{Pipeline, PipelineBindPoint};
use vulkano::query::QueryPool;
use vulkano::swapchain::{
    self, Surface, Swapchain, SwapchainAcquireFuture, SwapchainCreateInfo, SwapchainPresentInfo,
};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::sync::{self, GpuFuture, PipelineStage};
use vulkano::{Validated, VulkanError};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::camera::Camera;
use crate::engine::engine_parts::{EngineParts, ModelData};
use crate::engine::push_constants::PushConstants;
use crate::voxel_data::XYZIVoxelData;
use crate::voxel_loader::VoxFile;
use crate::vulkan::starter::{
    get_device_and_queue, get_headless_device_and_queue, get_headless_instance,
    get_physical_device_and_family_index, get_physical_device_and_family_index_for_surface,
    get_swapchain, get_windowed_instance,
};
use crate::vulkan::timings::get_query_timings;

type SwapchainFenceFuture = Arc<FenceSignalFuture<swapchain::PresentFuture<Box<dyn GpuFuture>>>>;
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

pub struct HeadlessEngine {
    descriptor_set: Arc<DescriptorSet>,
    fence: Option<FenceFuture>,
    output_image: Arc<Image>,
}

impl<T> Engine<T> {
    pub fn set_camera(&mut self, camera: Camera) {
        self.engine_parts.camera = camera;
        self.recompute_push_constants();
    }

    pub fn set_shader_flags(&mut self, flags: u8) {
        self.engine_parts.shader_flags = flags;
        self.recompute_push_constants();
    }

    fn recompute_push_constants(&mut self) {
        self.engine_parts.push_contants = PushConstants::new(
            &self.engine_parts.image_size,
            &self.engine_parts.camera,
            self.engine_parts.shader_flags,
            self.engine_parts.model_data.model_scale,
        );
    }

    /// Creates a new command buffer that:
    /// - resets the queries and creates 2 new timings if `should_write_timestamp` is true
    /// - dispatches the shader
    fn create_draw_to_image_command_buffer(
        // push_constants: PushConstants,
        descriptor_set: Arc<DescriptorSet>,
        // allocator: Arc<StandardCommandBufferAllocator>,
        output_image: &Arc<Image>,
        engine_parts: &EngineParts,
        // queue: &Arc<Queue>,
        // pipeline: Arc<ComputePipeline>,
        // query_pool: &Arc<QueryPool>,
        should_write_timestamp: bool,
        timestamp_index: u32,
    ) -> AutoCommandBufferBuilder<PrimaryAutoCommandBuffer> {
        let allocator = engine_parts.command_buffer_allocator.clone();
        let pipeline = engine_parts.compute_pipeline.clone();
        let mut builder = AutoCommandBufferBuilder::primary(
            allocator,
            engine_parts.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();
        let pipeline_layout = pipeline.layout().clone();
        let query_pool = &engine_parts.query_pool;

        builder
            .bind_pipeline_compute(pipeline)
            .unwrap()
            .push_constants(pipeline_layout.clone(), 0, engine_parts.push_contants)
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

    /// Creates the descriptor set of image views that are used by the shader. Also creates the output images that are written into by the shader
    fn create_descriptor_set_and_output_image(
        output_image_extent: &[u32; 2],
        model_data: &ModelData,
        allocator: Arc<StandardMemoryAllocator>,
        descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
        descriptor_set_layout: Arc<DescriptorSetLayout>,
    ) -> (Arc<DescriptorSet>, Arc<Image>) {
        let output_image = Image::new(
            allocator,
            ImageCreateInfo {
                format: Format::R8G8B8A8_UNORM,
                extent: [output_image_extent[0], output_image_extent[1], 1],
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
                WriteDescriptorSet::buffer(1, model_data.nodes.clone()),
                WriteDescriptorSet::buffer(2, model_data.leaf_data.clone()),
                WriteDescriptorSet::buffer(3, model_data.color_palette.clone()),
            ],
            [],
        )
        .unwrap();

        (descriptor_set, output_image)
    }
}

impl Engine<WindowedEngine> {
    pub fn new(resolution: [u32; 2], window: Arc<Window>, event_loop: &ActiveEventLoop) -> Self {
        let (renderer, engine_parts) =
            WindowedEngine::new_with_parts(resolution, window, event_loop);

        Self {
            engine_parts,
            renderer,
        }
    }

    pub fn draw(&mut self, window: &Arc<Window>) {
        self.renderer
            .draw::<fn(&mut Gui)>(&mut self.engine_parts, window, None);
    }

    pub fn draw_with_gui<RenderGuiFn>(&mut self, window: &Arc<Window>, gui_draw_fn: RenderGuiFn)
    where
        RenderGuiFn: FnOnce(&mut Gui),
    {
        self.renderer
            .draw(&mut self.engine_parts, window, Some(gui_draw_fn));
    }

    /// See docs for pass_event_to_gui
    pub fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.renderer.pass_event_to_gui(event)
    }

    pub fn image_draw_time_ns(&self) -> f64 {
        self.renderer.compute_time + self.renderer.copy_time
    }

    pub fn resize_output_image(&mut self, new_size: [u32; 2]) {
        self.engine_parts.image_size = new_size;
        self.renderer.handle_output_resize(&mut self.engine_parts);
    }

    pub fn resize_window(&mut self, window: &Arc<Window>) {
        self.renderer.handle_recreate_swapchain(window);
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

        let voxel_data = XYZIVoxelData::from_vox_file(
            VoxFile::load_vox_file(Path::new("models/tests/8 green corner cube.vox")).unwrap(),
        )
        .unwrap();

        let engine_parts = EngineParts::new(
            ModelData::new_from_vox_data(device.clone(), voxel_data),
            Camera::default(),
            output_image_size,
            &physical_device,
            device,
            queue,
            MAX_TIMESTAMP_QUERIES_PER_IMAGE,
        );

        let (descriptor_set, output_image) =
            Self::create_descriptor_set_and_output_image(&output_image_size, &engine_parts);

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

        let command_buffer = Engine::<Self>::create_draw_to_image_command_buffer(
            // engine_parts.push_contants,
            self.descriptor_set.clone(),
            // engine_parts.command_buffer_allocator.clone(),
            &self.output_image,
            &engine_parts,
            // &engine_parts.queue,
            // engine_parts.compute_pipeline.clone(),
            // &engine_parts.query_pool,
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

    fn create_descriptor_set_and_output_image(
        image_size: &[u32; 2],
        engine_parts: &EngineParts,
    ) -> (Arc<DescriptorSet>, Arc<Image>) {
        let pipeline_layout = engine_parts.compute_pipeline.layout();
        let descriptor_set_layout = pipeline_layout.set_layouts().first().unwrap();
        let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
            engine_parts.device.clone(),
            Default::default(),
        ));

        let allocator = Arc::new(StandardMemoryAllocator::new_default(
            engine_parts.device.clone(),
        ));

        Engine::<Self>::create_descriptor_set_and_output_image(
            image_size,
            &engine_parts.model_data,
            allocator,
            descriptor_set_allocator,
            descriptor_set_layout.clone(),
        )
    }
}

impl WindowedEngine {
    fn new_with_parts(
        resolution: [u32; 2],
        window: Arc<Window>,
        event_loop: &ActiveEventLoop,
    ) -> (Self, EngineParts) {
        let instance = get_windowed_instance(&window);
        let surface = Surface::from_window(instance.clone(), window.clone()).unwrap();

        let (physical_device, queue_family_index) =
            get_physical_device_and_family_index_for_surface(&surface, &instance);
        let (device, queue) = get_device_and_queue(physical_device.clone(), queue_family_index);

        let (swapchain, images) = get_swapchain(
            device.clone(),
            &physical_device,
            surface.clone(),
            window.inner_size().into(),
        );
        let voxel_data = XYZIVoxelData::from_vox_file(
            VoxFile::load_vox_file(Path::new("models/tests/8 green corner cube.vox")).unwrap(),
        )
        .unwrap();

        let engine_parts = EngineParts::new(
            ModelData::new_from_vox_data(device.clone(), voxel_data),
            Camera::default(),
            resolution,
            &physical_device,
            device,
            queue.clone(),
            images.len() as u32 * MAX_TIMESTAMP_QUERIES_PER_IMAGE,
        );

        let (descriptor_sets, output_images) =
            Self::create_descriptor_sets_and_output_images(images.len() as u32, &engine_parts);

        let images_and_views = Self::create_views_from_images(images);

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
        // window_resized: bool,
        engine_parts: &mut EngineParts,
        window: &Arc<Window>,
        gui_draw_fn: Option<RenderFn>,
    ) where
        RenderFn: FnOnce(&mut Gui),
    {
        // draw the gui before we have to wait for the fence. We will have to wait for the fence less
        if let Some(render_fn) = gui_draw_fn {
            self.gui.immediate_ui(render_fn);
        }

        // println!("Acquiring the next swapchaing image");
        let maybe_swapchain = self.acquire_next_swapchain_image(window);
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
                let mut now = sync::now(engine_parts.device.clone());
                now.cleanup_finished();
                now.boxed()
            }
            Some(fence) => fence.boxed(),
        };

        let command_buffer = Self::create_draw_to_swapchain_command_buffer(
            // engine_parts.push_contants,
            self.descriptor_sets[swap_image_index as usize].clone(),
            // engine_parts.command_buffer_allocator.clone(),
            self.present_images_and_views[swap_image_index as usize]
                .0
                .clone(),
            self.output_images[swap_image_index as usize].clone(),
            engine_parts,
            // &engine_parts.queue,
            // engine_parts.compute_pipeline.clone(),
            // engine_parts.query_pool.clone(),
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
            Ok(future) => Some(Arc::new(future)),
            Err(VulkanError::OutOfDate) => {
                self.handle_recreate_swapchain(window);
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

    fn handle_output_resize(&mut self, engine_parts: &mut EngineParts) {
        let (descriptor_sets, output_images) = Self::create_descriptor_sets_and_output_images(
            self.present_images_and_views.len() as u32,
            &engine_parts,
        );

        self.output_images = output_images;
        self.descriptor_sets = descriptor_sets;

        engine_parts.push_contants = PushConstants::new(
            &engine_parts.image_size,
            &engine_parts.camera,
            engine_parts.shader_flags,
            engine_parts.model_data.model_scale,
        );
    }

    fn handle_recreate_swapchain(&mut self, window: &Arc<Window>) {
        // println!("Recreating the swapchain");
        let (new_swapchain, new_images) = self
            .swapchain
            .recreate(SwapchainCreateInfo {
                image_extent: window.inner_size().into(),
                ..self.swapchain.create_info()
            })
            .unwrap();
        self.swapchain = new_swapchain;

        self.present_images_and_views = Self::create_views_from_images(new_images);
    }

    fn acquire_next_swapchain_image(
        &mut self,
        window: &Arc<Window>,
    ) -> Option<(u32, SwapchainAcquireFuture)> {
        // let now = Instant::now();
        let result =
            swapchain::acquire_next_image(self.swapchain.clone(), None).map_err(Validated::unwrap);

        let (swap_image_index, suboptimal_image, acquire_future) = match result {
            Ok(r) => r,
            Err(VulkanError::OutOfDate) => {
                self.handle_recreate_swapchain(window);
                return None;
            }
            Err(err) => panic!("{}", err),
        };

        if suboptimal_image {
            self.handle_recreate_swapchain(window);
        }

        Some((swap_image_index, acquire_future))
    }

    fn create_descriptor_sets_and_output_images(
        number_of_present_images: u32,
        engine_parts: &EngineParts,
    ) -> (Vec<Arc<DescriptorSet>>, Vec<Arc<Image>>) {
        let pipeline_layout = engine_parts.compute_pipeline.layout();
        let descriptor_set_layout = pipeline_layout.set_layouts().first().unwrap();
        let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
            engine_parts.device.clone(),
            Default::default(),
        ));

        let allocator = Arc::new(StandardMemoryAllocator::new_default(
            engine_parts.device.clone(),
        ));

        let mut descriptor_sets = Vec::new();
        let mut output_images = Vec::new();

        for _ in 0..number_of_present_images {
            let (descriptor_set, image) = Engine::<Self>::create_descriptor_set_and_output_image(
                &engine_parts.image_size,
                &engine_parts.model_data,
                allocator.clone(),
                descriptor_set_allocator.clone(),
                descriptor_set_layout.clone(),
            );
            descriptor_sets.push(descriptor_set);
            output_images.push(image);
        }
        (descriptor_sets, output_images)
    }

    fn create_views_from_images(images: Vec<Arc<Image>>) -> Vec<(Arc<Image>, Arc<ImageView>)> {
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

    // Gets the command buffer for a single dispatch of the compute shader.
    // This is done so that we can modify the push constants every frame.
    fn create_draw_to_swapchain_command_buffer(
        // push_constants: PushConstants,
        descriptor_set: Arc<DescriptorSet>,
        // allocator: Arc<StandardCommandBufferAllocator>,
        present_image: Arc<Image>,
        output_image: Arc<Image>,
        engine_parts: &EngineParts,
        // queue: &Arc<Queue>,
        // pipeline: Arc<ComputePipeline>,
        // query_pool: Arc<QueryPool>,
        swapchain_index: u32,
        should_write_timestamp: bool,
    ) -> Arc<PrimaryAutoCommandBuffer> {
        let timestamp_index = swapchain_index * MAX_TIMESTAMP_QUERIES_PER_IMAGE;

        let mut builder = Engine::<Self>::create_draw_to_image_command_buffer(
            // push_constants,
            descriptor_set,
            // allocator,
            &output_image,
            engine_parts,
            // queue,
            // pipeline,
            // &query_pool,
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
                    engine_parts.query_pool.clone(),
                    timestamp_index + 2,
                    PipelineStage::TopOfPipe,
                )
                .unwrap();
        }

        builder.build().unwrap()
    }
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
