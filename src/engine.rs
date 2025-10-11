use std::f32;
use std::sync::Arc;
use std::time::Duration;

use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferExecFuture, CommandBufferUsage, CopyBufferToImageInfo,
    CopyImageInfo, PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::{Device, Queue};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
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
use vulkano::shader::ShaderModule;
use vulkano::swapchain::{
    self, PresentFuture, Surface, Swapchain, SwapchainAcquireFuture, SwapchainCreateInfo,
    SwapchainPresentInfo,
};
use vulkano::sync::future::{FenceSignalFuture, JoinFuture};
use vulkano::{Validated, VulkanError};
use winit::window::Window;

use vulkano::sync::{self, GpuFuture};

use crate::vulkan::starter::{
    get_device_and_queue, get_instance, get_physical_device_and_family_index, get_swapchain,
};

pub struct Engine {
    recreate_swapchain: bool,

    // This is here so that we can modify it every frame by the app.
    // using a spherical coordinate system: https://en.m.wikipedia.org/wiki/Spherical_coordinate_system
    camera_x_radians: f32, // the angle off of the x vector
    camera_y_radians: f32, // the angle off of the z vector
    camera_radius: f32,

    descriptor_sets: Vec<Arc<DescriptorSet>>,
    present_images: Vec<Arc<Image>>,
    output_images: Vec<Arc<Image>>,
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    device: Arc<Device>,
    compute_pipeline: Arc<ComputePipeline>,
    swapchain: Arc<Swapchain>,
    queue: Arc<Queue>,
    previous_fence: usize,
    fences: Vec<
        Option<
            Arc<
                FenceSignalFuture<
                    PresentFuture<
                        CommandBufferExecFuture<
                            JoinFuture<Box<dyn GpuFuture>, SwapchainAcquireFuture>,
                        >,
                    >,
                >,
            >,
        >,
    >,
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct PushConstants {
    camera_position: [f32; 3],
}

impl Engine {
    pub fn new(window: Arc<Window>) -> Self {
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

        Self {
            recreate_swapchain: false,
            previous_fence: 0,
            device: device.clone(),
            compute_pipeline,
            swapchain,
            queue,
            fences: vec![None; images.len()],
            descriptor_sets,
            output_images,
            command_buffer_allocator: Arc::new(StandardCommandBufferAllocator::new(
                device.clone(),
                Default::default(),
            )),
            present_images: images,
            camera_x_radians: -f32::consts::FRAC_PI_2,
            camera_y_radians: f32::consts::FRAC_PI_2,
            camera_radius: 40.0,
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
                self.present_images = new_images;
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
            self.present_images[swap_image_index as usize].clone(),
            self.output_images[swap_image_index as usize].clone(),
            &self.queue,
            self.compute_pipeline.clone(),
        );

        let execution = previous_future
            .join(acquire_future)
            .then_execute(self.queue.clone(), command_buffer)
            .unwrap()
            .then_swapchain_present(
                self.queue.clone(),
                SwapchainPresentInfo::swapchain_image_index(
                    self.swapchain.clone(),
                    swap_image_index,
                ),
            )
            .then_signal_fence_and_flush();

        self.fences[swap_image_index as usize] = match execution.map_err(Validated::unwrap) {
            Ok(future) => Some(Arc::new(future)),
            Err(VulkanError::OutOfDate) => {
                self.recreate_swapchain = true;
                None
            }
            Err(err) => panic!("{err}"),
        };
        self.previous_fence = swap_image_index as usize;
    }

    pub fn move_horizontally(&mut self, amount_radians: f32) {
        self.camera_x_radians += amount_radians;
    }

    pub fn move_vertically(&mut self, amount_radians: f32) {
        self.camera_y_radians += amount_radians;
    }

    pub fn move_towards(&mut self, amount: f32) {
        self.camera_radius += amount;
    }
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

    let extent = present_image.extent();
    let local_size_in_shader = 8;

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

    builder
        .copy_image(CopyImageInfo::images(output_image, present_image))
        .unwrap();

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
                    format: present_image.format(),
                    extent: present_image.extent(),
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
    // for y in 0..diameter/2 {
    //     for x in 0..diameter  {
    //         let begin = y * diameter * bytes_per_texel + x*bytes_per_texel;
    //         data[begin..begin + bytes_per_texel].fill(255);
    //     }
    // }
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
                    data[begin] = 0xb8; // red;
                    data[begin + 1] = 0xbb; // green
                    data[begin + 2] = 0x26; // blue
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
