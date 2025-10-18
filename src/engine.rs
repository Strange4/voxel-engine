use std::f32;
use std::sync::Arc;
use std::time::Duration;

use egui::Align2;
use egui_winit_vulkano::{Gui, GuiConfig};
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BlitImageInfo, CommandBufferUsage, CopyBufferToImageInfo,
    PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
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
use vulkano::swapchain::{self, Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::{Validated, VulkanError};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use vulkano::sync::{self, GpuFuture, PipelineStage};

use crate::vulkan::starter::{
    get_device_and_queue, get_instance, get_physical_device_and_family_index, get_swapchain,
};

type CommandBufferFence = Arc<FenceSignalFuture<swapchain::PresentFuture<Box<dyn GpuFuture>>>>;

const TIMESTAMP_QUERIES_PER_IMAGE: u32 = 3;

// this records two uints per timestamp. One for results the other for availability
type TimestampAndAvailability = [u64; (TIMESTAMP_QUERIES_PER_IMAGE * 2) as usize];

pub struct Engine {
    recreate_swapchain: bool,

    // This is here so that we can modify it every frame by the app.
    // using a spherical coordinate system: https://en.m.wikipedia.org/wiki/Spherical_coordinate_system
    camera_x_radians: f32, // the angle off of the x vector
    camera_y_radians: f32, // the angle off of the z vector
    camera_radius: f32,

    // for doing swapchains
    device: Arc<Device>,
    swapchain: Arc<Swapchain>,
    queue: Arc<Queue>,
    previous_fence: usize,
    fences: Vec<Option<CommandBufferFence>>,
    present_images: Vec<(Arc<Image>, Arc<ImageView>)>,

    // stuff required for the compute shader
    descriptor_sets: Vec<Arc<DescriptorSet>>,
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    compute_pipeline: Arc<ComputePipeline>,
    output_images: Vec<Arc<Image>>,

    // for timings
    query_pool: Arc<QueryPool>,
    timing_period: f64,
    compute_time: f64,
    copy_time: f64,
    should_record: Vec<bool>,

    gui: Gui,
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct PushConstants {
    camera_position: [f32; 3],
}

impl Engine {
    pub fn new(window: Arc<Window>, event_loop: &ActiveEventLoop) -> Self {
        let instance = get_instance(&window);
        let surface = Surface::from_window(instance.clone(), window.clone()).unwrap();
        let dimensions = window.inner_size();

        let (physical_device, queue_family_index) =
            get_physical_device_and_family_index(&surface, &instance);
        let (device, queue) = get_device_and_queue(physical_device.clone(), queue_family_index);

        let (swapchain, images) = get_swapchain(
            device.clone(),
            &physical_device,
            surface.clone(),
            dimensions,
        );

        let compute_shader = cs::load(device.clone()).unwrap();
        let compute_pipeline = get_compute_pipeline(&device, &compute_shader);
        let (output_images, descriptor_sets) =
            create_descriptor_sets_and_output_images(&images, &compute_pipeline, &queue, &device);

        let query_pool = QueryPool::new(
            device.clone(),
            QueryPoolCreateInfo {
                query_count: images.len() as u32 * TIMESTAMP_QUERIES_PER_IMAGE,
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
            camera_x_radians: -f32::consts::FRAC_PI_2,
            camera_y_radians: f32::consts::FRAC_PI_2,
            camera_radius: 40.0,
            query_pool,
            timing_period: physical_device.properties().timestamp_period as f64,
            gui,
            compute_time: 0.0,
            copy_time: 0.0,
        }
    }

    pub fn draw(&mut self, window: &Arc<Window>, window_resized: bool) {
        if self.recreate_swapchain || window_resized {
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
            if window_resized {
                let (output_images, descriptor_sets) = create_descriptor_sets_and_output_images(
                    &new_images,
                    &self.compute_pipeline,
                    &self.queue,
                    &self.device,
                );
                self.present_images = get_images_and_views(new_images);
                self.output_images = output_images;
                self.descriptor_sets = descriptor_sets;
            }
        }
        let err =
            swapchain::acquire_next_image(self.swapchain.clone(), None).map_err(Validated::unwrap);

        let (swap_image_index, suboptimal_image, acquire_future) = match err {
            Ok(r) => r,
            Err(VulkanError::OutOfDate) => {
                self.recreate_swapchain = true;
                return;
            }
            Err(err) => panic!("{}", err),
        };

        if suboptimal_image {
            self.recreate_swapchain = true;
        }

        // draw the gui before we have to wait for the fence. We will have to wait for the fence less
        draw_gui(
            &mut self.gui,
            self.compute_time,
            self.copy_time
        );

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

        let sin = self.camera_y_radians.sin();

        let x = self.camera_radius * self.camera_x_radians.cos() * sin;
        let y = self.camera_radius * self.camera_y_radians.cos();
        let z = self.camera_radius * self.camera_x_radians.sin() * sin;

        let command_buffer = get_command_buffer(
            PushConstants {
                camera_position: [x, y, z],
            },
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
    }

    pub fn move_vertically(&mut self, amount_radians: f32) {
        self.camera_y_radians = (self.camera_y_radians + amount_radians).clamp(0.001, f32::consts::PI - 0.001);
    }

    pub fn move_towards(&mut self, amount: f32) {
        self.camera_radius += amount;
    }

    fn update_query_timings(&mut self, swap_image_index: u32) {
        let timestamp_index = swap_image_index * TIMESTAMP_QUERIES_PER_IMAGE;
        let mut timing_results: TimestampAndAvailability =
        [0; (TIMESTAMP_QUERIES_PER_IMAGE * 2) as usize];
        
        self.query_pool
            .get_results(
                timestamp_index..timestamp_index + TIMESTAMP_QUERIES_PER_IMAGE,
                &mut timing_results,
                QueryResultFlags::WITH_AVAILABILITY,
            )
            .unwrap();
        let all_available = !timing_results
            .iter()
            .enumerate()
            .any(|(index, &value)| index % 2 == 1 && value == 0);
        // update timings if all of them are available
        if all_available {
            let in_ms = self.timing_period / 1_000_000.0;
            self.compute_time = (timing_results[2] - timing_results[0]) as f64 * in_ms;
            self.copy_time = (timing_results[4] - timing_results[2]) as f64 * in_ms;
        }

        self.should_record[swap_image_index as usize] = all_available;
    }
}

fn draw_gui(
    gui: &mut Gui,
    compute_time: f64,
    copy_time: f64
) {

    gui.immediate_ui(|gui| {
        let ctx = gui.context();

        egui::Window::new("Specs")
            .anchor(Align2::LEFT_TOP, [5.0, 5.0])
            .auto_sized()
            .show(&ctx, |ui| {
                ui.label(format!("Compute time: {compute_time:.3}ms"));
                ui.label(format!("Copy time: {copy_time:.3}ms"));
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
fn get_command_buffer(
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

    let timestamp_index = swapchain_index * TIMESTAMP_QUERIES_PER_IMAGE;

    if should_write_timestamp {
        // safety: this the queries are not used in any other command buffer since there is no other command buffer
        unsafe {
            builder
                .reset_query_pool(
                    query_pool.clone(),
                    timestamp_index..timestamp_index + TIMESTAMP_QUERIES_PER_IMAGE,
                )
                .unwrap();
        }
    }

    if should_write_timestamp {
        // safety: reset query pool was done above
        unsafe {
            builder
                .write_timestamp(
                    query_pool.clone(),
                    timestamp_index,
                    PipelineStage::ComputeShader,
                )
                .unwrap();
        }
    }

    let extent = present_image.extent();
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

    if should_write_timestamp {
        // safety: reset query pool was done above
        unsafe {
            builder
                .write_timestamp(
                    query_pool.clone(),
                    timestamp_index + 1,
                    PipelineStage::AllTransfer,
                )
                .unwrap();
        }
    }

    builder
        .blit_image(BlitImageInfo::images(output_image, present_image))
        .unwrap();

    if should_write_timestamp {
        // safety: reset query pool was done above
        unsafe {
            builder
                .write_timestamp(
                    query_pool.clone(),
                    timestamp_index + 2,
                    PipelineStage::VertexInput,
                )
                .unwrap();
        }
    }

    builder.build().unwrap()
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
            let output_image = Image::new(
                allocator.clone(),
                ImageCreateInfo {
                    format: Format::R8G8B8A8_UNORM,
                    extent: present_image.extent(),
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
                descriptor_set_allocator.clone(),
                descriptor_set_layout.clone(),
                [
                    WriteDescriptorSet::image_view(0, output_image_view.clone()),
                    WriteDescriptorSet::image_view(1, model_image_view.clone()),
                ],
                [],
            )
            .unwrap();

            (output_image, descriptor_set)
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
