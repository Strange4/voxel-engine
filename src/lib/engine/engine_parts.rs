use std::sync::Arc;

use vulkano::{
    command_buffer::allocator::StandardCommandBufferAllocator,
    device::{Device, Queue, physical::PhysicalDevice},
    pipeline::{
        ComputePipeline, PipelineLayout, PipelineShaderStageCreateInfo,
        compute::ComputePipelineCreateInfo, layout::PipelineDescriptorSetLayoutCreateInfo,
    },
    query::{QueryPool, QueryPoolCreateInfo, QueryType},
    shader::ShaderModule,
};

use crate::{
    camera::Camera,
    engine::{compute_shader, push_constants::PushConstants},
};

pub struct EngineParts {
    pub camera: Camera,
    pub shader_flags: u8,

    // stuff that we want to precompute
    pub push_contants: PushConstants,
    pub image_size: [u32; 2],

    // Vulkan nececities
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,

    // stuff required for the compute shader
    pub command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    pub compute_pipeline: Arc<ComputePipeline>,

    // for timings
    pub query_pool: Arc<QueryPool>,
    pub timestamp_period: f64,
}

impl EngineParts {
    /// please input the TOTAL number_of_queries that the query pool can have
    /// If you have a swapchain and each image does a query, multiply them
    pub fn new(
        camera: Camera,
        output_image_size: [u32; 2],
        physical_device: &Arc<PhysicalDevice>,
        device: Arc<Device>,
        queue: Arc<Queue>,
        query_count: u32,
    ) -> EngineParts {
        let default_shader_flags = 0;
        let push_contants = PushConstants::new(&output_image_size, &camera, default_shader_flags);

        let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            Default::default(),
        ));
        let compute_shader = compute_shader::load(device.clone()).unwrap();
        let compute_pipeline = Self::create_compute_pipeline(&device, &compute_shader);

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
            shader_flags: default_shader_flags,
            push_contants,
            device,
            queue,
            command_buffer_allocator,
            compute_pipeline,
            query_pool,
            timestamp_period: physical_device.properties().timestamp_period as f64,
            image_size: output_image_size,
        }
    }

    fn create_compute_pipeline(
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
}
