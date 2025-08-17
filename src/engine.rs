use std::sync::Arc;

use vulkano::buffer::BufferContents;
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BlitImageInfo, CommandBufferExecFuture, CommandBufferUsage,
    PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::{Device, Queue};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::graphics::vertex_input::Vertex;
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

#[derive(BufferContents, Vertex)]
#[repr(C)]
struct MyVertex {
    #[format(R32G32_SFLOAT)]
    in_position: [f32; 2],

    #[format(R32G32B32_SFLOAT)]
    in_color: [f32; 3],
}

pub struct Engine {
    recreate_swapchain: bool,
    previous_fence: usize,
    command_buffers: Vec<Arc<PrimaryAutoCommandBuffer>>,
    device: Arc<Device>,
    compute_pipeline: Arc<ComputePipeline>,
    swapchain: Arc<Swapchain>,
    queue: Arc<Queue>,
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

        Self {
            recreate_swapchain: false,
            previous_fence: 0,
            command_buffers: get_compute_command_buffers(
                device.clone(),
                &queue,
                &compute_pipeline,
                &images,
            ),
            device,
            compute_pipeline,
            swapchain,
            queue,
            fences: vec![None; images.len()],
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
                self.command_buffers = get_compute_command_buffers(
                    self.device.clone(),
                    &self.queue,
                    &self.compute_pipeline,
                    &new_images,
                );
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
            self.recreate_swapchain = true
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

        let execution: Result<
            FenceSignalFuture<
                PresentFuture<
                    vulkano::command_buffer::CommandBufferExecFuture<
                        sync::future::JoinFuture<
                            Box<dyn GpuFuture>,
                            swapchain::SwapchainAcquireFuture,
                        >,
                    >,
                >,
            >,
            Validated<VulkanError>,
        > = previous_future
            .join(acquire_future)
            .then_execute(
                self.queue.clone(),
                self.command_buffers[swap_image_index as usize].clone(),
            )
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
            Err(err) => panic!("{}", err),
        };
        self.previous_fence = swap_image_index as usize;
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

fn get_compute_command_buffers(
    device: Arc<Device>,
    queue: &Arc<Queue>,
    pipeline: &Arc<ComputePipeline>,
    images: &[Arc<Image>],
) -> Vec<Arc<PrimaryAutoCommandBuffer>> {
    let pipeline_layout = pipeline.layout();
    let image_layout = pipeline_layout.set_layouts().get(0).unwrap();
    let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
        device.clone(),
        Default::default(),
    ));
    let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));

    let allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));

    // https://www.reddit.com/r/vulkan/comments/pf2no9/why_should_descriptor_sets_be_per_swap_chain_image/ you are supposed to have one descriptor set per image in swap cahin
    images
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
                image_layout.clone(),
                [WriteDescriptorSet::image_view(0, output_image_view.clone())],
                [],
            )
            .unwrap();

            let mut builder = AutoCommandBufferBuilder::primary(
                command_buffer_allocator.clone(),
                queue.queue_family_index(),
                CommandBufferUsage::MultipleSubmit,
            )
            .unwrap();

            let workgroup_size = 8;
            let extent = present_image.extent();

            builder
                .bind_pipeline_compute(pipeline.clone())
                .unwrap()
                .bind_descriptor_sets(
                    PipelineBindPoint::Compute,
                    pipeline_layout.clone(),
                    0,
                    descriptor_set,
                )
                .unwrap();

            // really no idea why this is unsafe now
            unsafe {
                builder
                    .dispatch([
                        (extent[0] + workgroup_size - 1) / workgroup_size,
                        (extent[1] + workgroup_size - 1) / workgroup_size,
                        1,
                    ])
                    .unwrap()
            };

            builder
                .blit_image(BlitImageInfo::images(
                    output_image.clone(),
                    present_image.clone(),
                ))
                .unwrap();

            builder.build().unwrap()
        })
        .collect::<Vec<_>>()
}

mod cs {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "shaders/main.comp"
    }
}
