use std::path::Path;

use voxel_engine::voxel_loader::{LoadVoxError, RGBAChunk, SizeChunk, VoxFile, XYZIChunk};

#[test]
fn test_header() -> Result<(), LoadVoxError> {
    let vox_path = Path::new("models/monu1.vox");
    let file = VoxFile::load_vox_file(vox_path)?;
    assert_eq!(150, file.version());
    Ok(())
}

#[test]
fn test_file_structure() -> Result<(), LoadVoxError> {
    let vox_path = Path::new("models/monu1.vox");
    let file = VoxFile::load_vox_file(vox_path)?;
    let main_chunk = file.main_chunk();

    assert_eq!("MAIN", main_chunk.name());

    let children = main_chunk.children();

    assert_eq!(3, children.len());

    let first_chunk = children.get(0).unwrap();
    let second_chunk = children.get(1).unwrap();
    let third_chunk = children.get(2).unwrap();

    assert_eq!("SIZE", first_chunk.name());
    assert_eq!("XYZI", second_chunk.name());
    assert_eq!("RGBA", third_chunk.name());

    let size_chunk: SizeChunk = first_chunk
        .try_into()
        .expect("Couldn't transform chunk into a size chunk");

    assert_eq!(126, size_chunk.x());
    assert_eq!(126, size_chunk.y());
    assert_eq!(118, size_chunk.z());

    let xyzi_chunk: XYZIChunk = second_chunk
        .try_into()
        .expect("Couldn't transform into an xyzi chunk");

    assert_eq!(156942, xyzi_chunk.voxels().len());

    let _: RGBAChunk = third_chunk
        .try_into()
        .expect("Couldn't transform chunk into a rgba chunk");
    Ok(())
}
