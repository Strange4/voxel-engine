use glam::UVec3;
use std::sync::Arc;
use vulkano::{
    buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer},
    command_buffer::allocator::StandardCommandBufferAllocator,
    device::{Device, Queue, physical::PhysicalDevice},
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
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
    svt::{Svt, SvtNode},
    voxel_data::XYZIVoxelData,
    voxel_loader::DEFAULT_PALETTE,
};

pub struct EngineParts {
    pub camera: Camera,
    pub shader_flags: u8,

    // stuff that we want to precompute
    pub push_constants: PushConstants,
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

    pub model_data: ModelData,
}

impl EngineParts {
    /// please input the TOTAL number_of_queries that the query pool can have
    /// If you have a swapchain and each image does a query, multiply them
    pub fn new(
        model_data: ModelData,
        camera: Camera,
        output_image_size: [u32; 2],
        physical_device: &Arc<PhysicalDevice>,
        device: Arc<Device>,
        queue: Arc<Queue>,
        query_count: u32,
    ) -> EngineParts {
        let default_shader_flags = 0;
        let push_contants = PushConstants::new(
            &output_image_size,
            &camera,
            default_shader_flags,
            &model_data,
        );

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
            push_constants: push_contants,
            device,
            queue,
            command_buffer_allocator,
            compute_pipeline,
            query_pool,
            timestamp_period: physical_device.properties().timestamp_period as f64,
            image_size: output_image_size,
            model_data,
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

pub struct ModelData {
    pub nodes: Subbuffer<[SvtNode]>,
    pub leaf_data: Subbuffer<[u8]>,
    pub color_palette: Subbuffer<[u32]>,
    pub model_scale: u8,
    pub floor_position: u8,
}

impl ModelData {
    pub fn new(
        device: Arc<Device>,
        svt: Svt,
        color_palette: [u32; 256],
        floor_position: u8,
    ) -> Self {
        let allocator = Arc::new(StandardMemoryAllocator::new_default(device));

        let nodes_buffer = Buffer::from_iter(
            allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            svt.node_pool,
        )
        .expect("Couldn't create buffer");

        let leaf_data_buffer = Buffer::from_iter(
            allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            svt.leaf_data,
        )
        .expect("Couldn't create buffer");

        // We could resize the entire color pallete to only the colors that are used to save memory
        // But its 1KB anyway so doesn't matter much
        let color_palette_buffer = Buffer::from_iter(
            allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::UNIFORM_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            color_palette,
        )
        .expect("Couldn't create buffer");

        Self {
            nodes: nodes_buffer,
            leaf_data: leaf_data_buffer,
            color_palette: color_palette_buffer,
            model_scale: svt.scale,
            floor_position,
        }
    }

    pub fn new_from_vox_data(device: Arc<Device>, mut voxel_data: XYZIVoxelData) -> Self {
        let svt = Svt::from_voxel_data(&mut voxel_data);
        // Cap to 255 since we only load one object and the max size of an object is 255
        let height = voxel_data.size().y.min(255) as u8;

        let color_palette = voxel_data.color_palette();

        Self::new(device, svt, color_palette, height)
    }

    pub fn sample_model(device: Arc<Device>) -> Self {
        // its kinda like a sphere
        #[rustfmt::skip]
        const SAMPLE_VOXEL_DATA: [u8; 64] = [
            0, 0, 0, 0,
            0, 1, 1, 0,
            0, 1, 1, 0,
            0, 0, 0, 0,

            0, 1, 1, 0,
            1, 1, 1, 1,
            1, 1, 1, 1,
            0, 1, 1, 0,

            0, 1, 1, 0,
            1, 1, 1, 1,
            1, 1, 1, 1,
            0, 1, 1, 0,

            0, 0, 0, 0,
            0, 1, 1, 0,
            0, 1, 1, 0,
            0, 0, 0, 0,
        ];
        const SAMPLE_SIZE: UVec3 = UVec3::new(4, 4, 4);
        let svt = Svt::build_tree(&SAMPLE_SIZE, &SAMPLE_VOXEL_DATA);
        Self::new(device, svt, DEFAULT_PALETTE, SAMPLE_SIZE.y as u8)
    }
}
