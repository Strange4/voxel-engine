use std::path::Path;

use glam::UVec3;
use voxel_engine::{svt::Svt, voxel_data::XYZIVoxelData, voxel_loader::VoxFile};

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
fn test_build_tree_level1() {
    let tree = Svt::build_tree(&SAMPLE_SIZE, &SAMPLE_VOXEL_DATA);

    // There's only 32 voxels that have some data
    assert_eq!(tree.leaf_data.len(), 32);
    assert_eq!(tree.leaf_data.as_slice(), &[1; 32]);

    // There should only be one node (the root) since it fits in a single 4^3 box.
    assert_eq!(tree.node_pool.len(), 1);

    let root_node = &tree.node_pool[0];

    assert_eq!(root_node.is_leaf(), true);

    // Since there is only one node in this tree and it is a leaf,
    // the leaf data should point to the start of the leaf data vector
    assert_eq!(root_node.child_pointer(), 0);

    const WANTED_MASK: u64 =
        0b0000_0110_0110_0000_0110_1111_1111_0110_0110_1111_1111_0110_0000_0110_0110_0000;
    assert_eq!(root_node.child_mask(), WANTED_MASK);
}

#[test]
fn test_build_tree_level2() {
    // This file contains an 16^3 model with 8 green voxels at each corner of the box
    let vox_file =
        VoxFile::load_vox_file(Path::new("models/tests/8 green corner cube.vox")).unwrap();
    let mut voxel_data = XYZIVoxelData::from_vox_file(vox_file).unwrap();
    let tree = Svt::from_voxel_data(&mut voxel_data);
    let color_pallete: &[u32; 256] = voxel_data.color_pallete();

    // There's 8 voxels in this file
    assert_eq!(tree.leaf_data.len(), 8);
    assert_eq!(color_pallete[tree.leaf_data[0] as usize], 0x00_FF_00_FF);

    // Since the model is 16^3, there should only be two levels
    // and because there are only 8 voxels in level 1, the total number of nodes should be 8 + 1 (top level)
    assert_eq!(tree.node_pool.len(), 9);

    let root_node = &tree.node_pool[0];
    assert!(!root_node.is_leaf());

    // The root's first child should be the first element after the root (aka index 1 since root is index 0)
    assert_eq!(root_node.child_pointer(), 1);

    // Only the corners of the top level node should have active children
    let root_child_mask = root_node.child_mask();
    const ROOT_WANTED_MASK: u64 =
        0b1001_0000_0000_1001_0000_0000_0000_0000_0000_0000_0000_0000_1001_0000_0000_1001;
    assert_eq!(root_child_mask, ROOT_WANTED_MASK);

    let root_child_pointer = root_node.child_pointer();
    for i in 0..64 {
        if root_child_mask >> i & 1 != 1 {
            // No child to see here
            continue;
        }
        let number_of_children_before = (root_child_mask & ((1 << i) - 1)).count_ones() as u32;
        let child_index = root_child_pointer + number_of_children_before;
        let child = &tree.node_pool[child_index as usize];

        // There's only 2 levels, so if you are a child of the root you must be a leaf
        assert!(child.is_leaf());
        assert!(child.has_children());
    }

    const CHILD_WANTED_MASK: u64 =
        0b0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0001;

    let child = &tree.node_pool[1];
    assert_eq!(child.child_mask(), CHILD_WANTED_MASK);
}
