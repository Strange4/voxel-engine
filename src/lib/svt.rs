// from: https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing/

use glam::{USizeVec3, UVec3};
use vulkano::buffer::BufferContents;

use crate::voxel_data::XYZIVoxelData;

#[repr(C)]
#[derive(BufferContents, Debug, Default)]
pub struct SvtNode {
    is_leaf_and_child_pointer: u32,
    child_mask_high: u32,
    child_mask_low: u32,
}

impl SvtNode {
    const WITHOUT_IS_LEAF_MASK: u32 = 0x7FFFFFFF;

    #[inline]
    pub fn has_children(&self) -> bool {
        self.child_mask_high != 0 || self.child_mask_low != 0
    }

    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.is_leaf_and_child_pointer >> 31 == 1
    }

    #[inline]
    pub fn child_pointer(&self) -> u32 {
        self.is_leaf_and_child_pointer & Self::WITHOUT_IS_LEAF_MASK
    }

    #[inline]
    fn set_child_pointer(&mut self, child_pointer: u32) {
        assert!(child_pointer <= Self::WITHOUT_IS_LEAF_MASK);
        self.is_leaf_and_child_pointer |= child_pointer;
    }

    #[inline]
    fn mark_as_leaf(&mut self) {
        self.is_leaf_and_child_pointer |= 1 << 31;
    }

    #[inline]
    pub fn child_mask(&self) -> u64 {
        (self.child_mask_high as u64) << 32 | self.child_mask_low as u64
    }

    #[inline]
    fn set_child_mask(&mut self, mask: u64) {
        self.child_mask_low = mask as u32;
        self.child_mask_high = (mask >> 32) as u32;
    }

    #[inline]
    fn mark_child_active(&mut self, child_index: u8) {
        assert!(child_index <= 64);
        if child_index > 31 {
            self.child_mask_high |= 1 << (child_index - 32);
        } else {
            self.child_mask_low |= 1 << child_index;
        }
    }
}

pub struct Svt {
    pub node_pool: Vec<SvtNode>,
    pub leaf_data: Vec<u8>,
    pub scale: u8,
}

impl Svt {
    /// Creates a Sparse Voxel Tree representation of the voxel data.
    pub fn from_voxel_data(voxel_data: &mut XYZIVoxelData) -> Self {
        let data = voxel_data.as_palette_indices();
        let size = voxel_data.size();
        Self::build_tree(size, &data)
    }

    pub fn build_tree(model_size: &UVec3, voxel_bytes: &[u8]) -> Self {
        // make the space for the root to be added at the 0'th spot
        let mut node_pool = vec![SvtNode::default()];
        let mut leaf_data = Vec::new();

        // We have to make the tree as big as the max side of the model.
        // What is the height of the tree with 4 children at each level and n number of children?
        let scale = (model_size.max_element() - 1).ilog(4) + 1; // Same as log(x as f32, base = 4.0).ceil();

        assert!(scale <= 11);

        let root = Self::build_tree_recursive(
            &mut node_pool,
            &mut leaf_data,
            model_size,
            voxel_bytes,
            UVec3::ZERO,
            scale,
        );

        node_pool[0] = root;

        Self {
            node_pool,
            leaf_data,
            scale: scale as u8, // This won't be more than 16 since the max width of a model is 2^32
        }
    }

    fn build_tree_recursive(
        node_pool: &mut Vec<SvtNode>,
        leaf_data: &mut Vec<u8>,
        model_size: &UVec3,
        voxel_bytes: &[u8],
        position: UVec3,
        mut scale: u32,
    ) -> SvtNode {
        let mut node = SvtNode::default();

        // build leaf
        if scale == 1 {
            // make sure that all coordinates are a multiple of 4
            assert!((position.x | position.y | position.z) % 4 == 0);

            // 4^3 cube of data
            let brick = Self::get_brick(model_size, voxel_bytes, &position);

            // if there is no data, this node won't be added to its parent so might as well return now
            if brick.is_none() {
                return node;
            }

            node.mark_as_leaf();

            let mut brick = brick.unwrap();

            let child_mask = Self::child_mask(&brick);
            node.set_child_mask(child_mask);

            Self::pack_brick_tightly(&mut brick, child_mask);

            // Set the child pointer to the actual leaf data
            node.set_child_pointer(leaf_data.len() as u32);
            let number_of_elements = child_mask.count_ones() as usize;
            leaf_data.extend_from_slice(&brick[0..number_of_elements]);

            return node;
        }

        // if we're not at the ground scale we descend
        scale -= 1;

        let mut children = Vec::new();

        for i in 0..64 {
            // we fill the data in the x,y,z order
            let x = i & 3;
            let y = i >> 2 & 3;
            let z = i >> 4 & 3;
            // Each scale level we move by 4^scale. We use bit shift to do the multiplication faster
            let child_offset = UVec3::new(x, y, z) << (2 * scale);
            let child_position = position + child_offset;

            let child = Self::build_tree_recursive(
                node_pool,
                leaf_data,
                model_size,
                voxel_bytes,
                child_position,
                scale,
            );

            // This child has data, let's keep it
            if child.has_children() {
                node.mark_child_active(i as u8);
                children.push(child);
            }
        }

        node.set_child_pointer(node_pool.len() as u32);
        assert!(node_pool.len() <= 0x7F_FF_FF_FF);
        node_pool.append(&mut children);

        node
    }

    fn get_brick(model_size: &UVec3, voxel_bytes: &[u8], position: &UVec3) -> Option<[u8; 64]> {
        if position.x >= model_size.x || position.y >= model_size.y || position.z >= model_size.z {
            return None;
        }

        let size: USizeVec3 = USizeVec3::new(
            model_size.x as usize,
            model_size.y as usize,
            model_size.z as usize,
        );
        let position = USizeVec3::new(
            position.x as usize,
            position.y as usize,
            position.z as usize,
        );

        let distance_to_end = size - position;
        let mut brick = [0; 64];
        let copy_size = distance_to_end.min(USizeVec3::new(4, 4, 4));

        for z in 0..copy_size.z {
            for y in 0..copy_size.y {
                let source_start_index =
                    position.x + (position.y + y) * size.x + (position.z + z) * size.x * size.y;
                let destination_start_index = 0 + y * 4 + z * 16;
                brick[destination_start_index..destination_start_index + copy_size.x]
                    .copy_from_slice(
                        &voxel_bytes[source_start_index..source_start_index + copy_size.x],
                    );
            }
        }

        Some(brick)
    }

    fn child_mask(brick: &[u8; 64]) -> u64 {
        let mut mask = 0;
        for (i, value) in brick.iter().enumerate().take(64) {
            mask |= ((*value != 0) as u64) << i;
        }
        mask
    }

    fn pack_brick_tightly(brick: &mut [u8; 64], mut mask: u64) {
        let mut j = 0;
        for i in 0..64 {
            if mask & 1 != 0 {
                brick[j] = brick[i];
                j += 1;
            }
            mask >>= 1;
        }
    }
}

#[cfg(test)]
mod test_svt {
    use glam::UVec3;

    use super::Svt;

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

    #[test]
    fn test_child_mask() {
        let mut brick = [0; 64];
        brick[0] = 1;
        brick[3] = 255;

        let mask = Svt::child_mask(&brick);

        assert_eq!(mask, 0b1001);
    }

    #[test]
    fn test_pack_brick_rightly() {
        let mut brick = [0; 64];
        brick[3] = 1;
        brick[5] = 144;
        brick[6] = 3;

        let mask = Svt::child_mask(&brick);

        assert_eq!(mask, 0b1101000);

        Svt::pack_brick_tightly(&mut brick, mask);

        assert_eq!(brick[0], 1);
        assert_eq!(brick[1], 144);
        assert_eq!(brick[2], 3);
    }

    #[test]
    fn test_get_brick_fully_inside() {
        let brick = Svt::get_brick(&SAMPLE_SIZE, &SAMPLE_VOXEL_DATA, &UVec3::ZERO).unwrap();

        assert_eq!(brick, SAMPLE_VOXEL_DATA);
    }

    #[test]
    fn test_get_brick_halfway_inside() {
        #[rustfmt::skip]
        const EXPECTED: [u8; 64] = [
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

            0, 0, 0, 0,
            0, 0, 0, 0,
            0, 0, 0, 0,
            0, 0, 0, 0,
        ];

        let sample_position = UVec3::new(0, 0, 1);

        // if we sample at z position 1, it should still return a brick of 64 size but the places that there is no data it should
        // just return 0
        let brick = Svt::get_brick(&SAMPLE_SIZE, &SAMPLE_VOXEL_DATA, &sample_position).unwrap();

        assert_eq!(brick, EXPECTED);
    }

    #[test]
    fn test_get_brick_different_size() {
        #[rustfmt::skip]
        const DATA: [u8; 48] = [
            0, 1, 1,
            1, 1, 1,
            1, 1, 1,
            0, 1, 1,

            0, 1, 1,
            1, 1, 1,
            1, 1, 1,
            0, 1, 1,

            0, 0, 0,
            0, 1, 1,
            0, 1, 1,
            0, 0, 0,

            0, 0, 0,
            0, 0, 0,
            0, 0, 0,
            0, 0, 0,
        ];

        const DATA_SIZE: UVec3 = UVec3::new(3, 4, 4);

        #[rustfmt::skip]
        const EXPECTED: [u8; 64] = [
            0, 1, 1, 0,
            1, 1, 1, 0,
            1, 1, 1, 0,
            0, 1, 1, 0,

            0, 1, 1, 0,
            1, 1, 1, 0,
            1, 1, 1, 0,
            0, 1, 1, 0,

            0, 0, 0, 0,
            0, 1, 1, 0,
            0, 1, 1, 0,
            0, 0, 0, 0,

            0, 0, 0, 0,
            0, 0, 0, 0,
            0, 0, 0, 0,
            0, 0, 0, 0,
        ];

        // If we sample and the data is smaller than a 64 block, we should return zeroes for the rest
        let brick = Svt::get_brick(&DATA_SIZE, &DATA, &UVec3::ZERO).unwrap();

        assert_eq!(brick, EXPECTED);
    }
}

#[cfg(test)]
mod test_svt_node {

    use super::SvtNode;

    #[test]
    fn test_setting_masks_and_pointers() {
        let mut node = SvtNode::default();

        node.mark_as_leaf();
        assert_eq!(node.is_leaf_and_child_pointer, 1 << 31);

        node.set_child_pointer(33);
        assert_eq!(node.is_leaf_and_child_pointer, (1 << 31) | 33);

        node.mark_child_active(3);
        assert_eq!(node.child_mask_high, 0);
        assert_eq!(node.child_mask_low, 0b1000);

        assert!(node.has_children());
    }
}
