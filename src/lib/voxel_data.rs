use glam::{UVec3, uvec3};

use crate::voxel_loader::{DEFAULT_PALETTE, RGBAChunk, SizeChunk, VoxFile, XYZIChunk, XYZIVoxel};

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

        let max_voxel_position = xyzi_chunk
            .voxels()
            .iter()
            .max_by(|&a, &b| a.xyz().length_squared().cmp(&b.xyz().length_squared()));

        let lowest_voxel = xyzi_chunk
            .voxels()
            .iter()
            .min_by(|&a, &b| a.y().cmp(&b.y()));

        // Verify that some voxels aren't out of bounds
        if let Some(voxel) = max_voxel_position
            && ((voxel.x() as u32) > size_chunk.x()
                || (voxel.y() as u32) > size_chunk.y()
                || (voxel.z() as u32) > size_chunk.z())
        {
            return Err(VoxelDataError::VoxelOutOfBounds);
        };

        let mut color_palette = if let Some(rgba_chunk) = main_chunk.find_child("RGBA") {
            let chunk: RGBAChunk = rgba_chunk
                .try_into()
                .map_err(|e| VoxelDataError::TransformChunkError(format!("{e:?}")))?;

            chunk.get_palette()
        } else {
            DEFAULT_PALETTE
        };

        if let Some(voxel) = lowest_voxel {
            color_palette[0] = color_palette[voxel.i() as usize];
        } else {
            color_palette[0] = 0xb2c553ff;
        }

        Ok(Self {
            // the Y and Z axis are inversed from the .vox file format and the vulkan coordinate system
            size: uvec3(size_chunk.x(), size_chunk.z(), size_chunk.y()),
            data: xyzi_chunk.get_voxels(),
            color_palette,
        })
    }

    /// Consumes the voxel data and creates a 3d texture data for RGBA bytes of the size of the voxel data.
    /// The vox XYZ coordinates are mapped to Vulkan NDC
    /// The bytes are filled in the x, y, z order
    pub fn as_rgba_bytes(&mut self) -> Vec<u8> {
        let bytes_per_voxel = 4;
        let (x_size, y_size, z_size) = (
            self.size.x as usize,
            self.size.y as usize,
            self.size.z as usize,
        );

        let number_of_voxels = x_size * y_size * z_size;
        let mut bytes = vec![0; bytes_per_voxel * number_of_voxels];
        self.data.drain(..).for_each(|voxel| {
            let (x, y, z) = Self::to_ndc(&self.size, &voxel);

            let start_index = z * x_size * y_size * bytes_per_voxel
                + y * x_size * bytes_per_voxel
                + x * bytes_per_voxel;

            let color = self.color_palette[voxel.i() as usize];
            bytes[start_index] = ((color & 0xFF000000) >> 24) as u8; // red;
            bytes[start_index + 1] = ((color & 0x00FF0000) >> 16) as u8; // blue;
            bytes[start_index + 2] = ((color & 0xFF00) >> 8) as u8; // green;
            bytes[start_index + 3] = (color & 0xFF) as u8; // alpha
        });
        self.data.shrink_to_fit();
        bytes
    }

    /// Consumes the voxel data and creates a 3d representation of the collor palette indices.
    /// The vox XYZ coordinates are mapped to Vulkan NDC
    /// The bytes are filled in the x, y, z order
    pub fn as_palette_indices(&mut self) -> Vec<u8> {
        let (x_size, y_size, z_size) = (
            self.size.x as usize,
            self.size.y as usize,
            self.size.z as usize,
        );

        let number_of_voxels = x_size * y_size * z_size;
        let mut bytes = vec![0; number_of_voxels];
        self.data.drain(..).for_each(|voxel| {
            let (x, y, z) = Self::to_ndc(&self.size, &voxel);

            let index = z * x_size * y_size + y * x_size + x;
            bytes[index] = voxel.i()
        });
        bytes
    }

    fn to_ndc(size: &UVec3, voxel: &XYZIVoxel) -> (usize, usize, usize) {
        let y_size = size.y as usize;
        let (x, y, z) = (voxel.x() as usize, voxel.y() as usize, voxel.z() as usize);
        // The xyz coordinates of the .vox format aren't the same as vulkan's normalized device coordinates
        // The z is up the screen in .vox instead towards inward like in NDC
        let (y, z) = (y_size - z - 1, y);
        (x, y, z)
    }

    pub fn size(&self) -> &UVec3 {
        &self.size
    }

    pub fn color_palette(self) -> [u32; 256] {
        self.color_palette
    }
}
