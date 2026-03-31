use glam::{U8Vec3, UVec3, Vec3, u8vec3, uvec3, vec3};

use crate::voxel_loader::{
    RGBAChunk, SizeChunk, TransformChunkError, VoxFile, XYZIChunk, XYZIVoxel,
};

pub struct XYZIVoxelData {
    // Size of the entire voxel model, might not reflect the exact boundary box of the voxels
    size: UVec3,
    data: Vec<XYZIVoxel>,
    color_palette: [u32; 256],
}

#[derive(Debug)]
pub enum VoxelDataError {
    ChunkNotFound(String),
    TransformChunkError(String),
    VoxelOutOfBounds,
}

impl XYZIVoxelData {
    pub fn from_vox_file(file: VoxFile) -> Result<Self, VoxelDataError> {
        let main_chunk = file.main_chunk();

        // This gets the first size chunk, but there might be many in an animation file
        // Its okay, I only want to render the first frame
        let size_chunk: SizeChunk = main_chunk
            .find_child("SIZE")
            .ok_or(VoxelDataError::ChunkNotFound("SIZE".to_string()))?
            .try_into()
            .map_err(|e| VoxelDataError::TransformChunkError(format!("{e:?}")))?;

        // This gets the first xyzi chunk, it doesn't know if its the one right after the above size chunk.
        // If the file is badly formatted, we'll get the wrong dimentions and we might not fit
        let xyzi_chunk: XYZIChunk = main_chunk
            .find_child("XYZI")
            .ok_or(VoxelDataError::ChunkNotFound("XYZI".to_string()))?
            .try_into()
            .map_err(|e| VoxelDataError::TransformChunkError(format!("{e:?}")))?;

        let max_voxel_position = xyzi_chunk.voxels().iter().max_by(|&a, &b| {
            let a_distance = a.x() as u16 * a.x() as u16
                + a.y() as u16 * a.y() as u16
                + a.z() as u16 * a.z() as u16;
            let b_distbnce = b.x() as u16 * b.x() as u16
                + b.y() as u16 * b.y() as u16
                + b.z() as u16 * b.z() as u16;
            a_distance.cmp(&b_distbnce)
        });

        // Verify that some voxels aren't out of bounds
        if let Some(voxel) = max_voxel_position {
            if (voxel.x() as u32) > size_chunk.x()
                || (voxel.y() as u32) > size_chunk.y()
                || (voxel.z() as u32) > size_chunk.z()
            {
                return Err(VoxelDataError::VoxelOutOfBounds);
            }
        };

        let rgba_chunk: RGBAChunk = main_chunk
            .find_child("RGBA")
            .ok_or(VoxelDataError::ChunkNotFound("RGBA".to_string()))?
            .try_into()
            .map_err(|e| VoxelDataError::TransformChunkError(format!("{e:?}")))?;

        Ok(Self {
            size: uvec3(size_chunk.x(), size_chunk.y(), size_chunk.z()),
            data: xyzi_chunk.get_voxels(),
            color_palette: rgba_chunk.get_palette(),
        })
    }

    pub fn as_rgba_bytes(&self) -> Vec<u8> {
        let bytes_per_voxel = 4;
        let (x_size, y_size, z_size) = (
            self.size.x as usize,
            self.size.y as usize,
            self.size.z as usize,
        );
        let number_of_voxels = x_size * y_size * z_size;
        let mut bytes = vec![0; bytes_per_voxel * number_of_voxels];
        self.data.iter().for_each(|voxel| {
            let (x, y, z) = (voxel.x() as usize, voxel.y() as usize, voxel.z() as usize);
            // The xyz coordinates of the .vox format aren't the same as vulkan's normalized device coordinates
            // The z is up the screen in .vox instead towards inward like in NDC
            let (y, z) = (y_size - z - 1, y);

            let start_index = z * x_size * y_size * bytes_per_voxel
                + y * x_size * bytes_per_voxel
                + x * bytes_per_voxel;
            let color = self.color_palette[voxel.i() as usize];
            bytes[start_index] = ((color & 0xFF0000) >> 16) as u8; // red;
            bytes[start_index + 1] = ((color & 0x00FF00) >> 8) as u8; // blue;
            bytes[start_index + 2] = (color & 0x0000FF) as u8; // green;
            bytes[start_index + 3] = 0xFF; // alpha
        });
        bytes
    }

    pub fn size(&self) -> &UVec3 {
        &self.size
    }
}
