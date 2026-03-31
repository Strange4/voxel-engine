use std::{fmt::Debug, fs::File, io::Read, path::Path};

#[derive(Debug)]
pub enum LoadVoxError {
    CouldNotOpenFile,
    CouldNotReadFile,
    InvalidFileHeader,
    InvalidChunkName,
    NoMainChunk,
}

#[derive(Debug)]
pub enum TransformChunkError {
    WrongTagName,
    UnexpectedChildren,
    UnexpectedContentSize,
}

pub struct VoxFile {
    version_number: u32,
    main_chunk: VoxChunk,
}

pub struct VoxChunk {
    tag_name: String,
    content: Vec<u8>,
    children: Vec<VoxChunk>,
}

pub struct SizeChunk {
    tag_name: String,
    x_size: u32,
    y_size: u32,
    z_size: u32,
}

pub struct XYZIChunk {
    tag_name: String,
    voxels: Vec<XYZIVoxel>,
}

pub struct RGBAChunk {
    rgba_palette: [u32; 256],
}

pub struct PackChunk {
    number_of_models: u32,
}

pub struct XYZIVoxel {
    x: u8,
    y: u8,
    z: u8,
    i: u8,
}

impl VoxFile {
    pub fn load_vox_file(path: &Path) -> Result<Self, LoadVoxError> {
        let mut file = File::open(path).map_err(|_| LoadVoxError::CouldNotOpenFile)?;
        let version_number = Self::read_file_header(&mut file)?;

        let mut main_chunk = Self::read_chunk_recursive(&mut file)?;
        if main_chunk.tag_name != "MAIN" {
            return Err(LoadVoxError::NoMainChunk);
        }
        let no_palette = main_chunk
            .children
            .iter()
            .find(|&chunk| chunk.tag_name == "RGBA")
            .is_none();
        if no_palette {
            main_chunk.children.push(
                RGBAChunk {
                    rgba_palette: DEFAULT_PALETTE,
                }
                .into(),
            );
        }
        Ok(Self {
            version_number,
            main_chunk,
        })
    }

    fn read_file_header(file: &mut File) -> Result<u32, LoadVoxError> {
        let mut header_buffer = [0; 4];
        file.read_exact(&mut header_buffer)
            .map_err(|_| LoadVoxError::CouldNotReadFile)?;

        let magic_bytes = String::from_utf8(header_buffer.to_vec())
            .map_err(|_| LoadVoxError::InvalidFileHeader)?;

        if magic_bytes != "VOX " {
            return Err(LoadVoxError::InvalidFileHeader);
        }

        Self::read_into(file, &mut header_buffer)?;
        let version_number = u32::from_le_bytes(header_buffer);

        Ok(version_number)
    }

    pub fn version(&self) -> u32 {
        self.version_number
    }

    pub fn main_chunk(&self) -> &VoxChunk {
        &self.main_chunk
    }

    fn read_chunk_recursive(file: &mut File) -> Result<VoxChunk, LoadVoxError> {
        let mut buf = [0; 4];

        // Read tag
        Self::read_into(file, &mut buf)?;
        let tag_name =
            String::from_utf8(buf.to_vec()).map_err(|_| LoadVoxError::InvalidChunkName)?;

        // read content size
        Self::read_into(file, &mut buf)?;
        let chunk_content_size = u32::from_le_bytes(buf);

        // read the size of the child
        Self::read_into(file, &mut buf)?;
        let mut children_content_size = u32::from_le_bytes(buf);

        // read the actual content
        let mut content = vec![0; chunk_content_size as usize];
        Self::read_into(file, &mut content)?; // If I was more careful I would not read an arbitrary amount of bytes from user input

        // read the content of the children
        let mut children = vec![];
        while children_content_size > 0 {
            let child = Self::read_chunk_recursive(file)?;
            let header_bytes = 12;
            let child_original_size = child.content.len() + child.children.len() + header_bytes;
            children_content_size -= child_original_size as u32;
            children.push(child);
        }

        Ok(VoxChunk {
            tag_name,
            content,
            children,
        })
    }

    fn read_into(file: &mut File, buf: &mut [u8]) -> Result<(), LoadVoxError> {
        file.read_exact(buf)
            .map_err(|_| LoadVoxError::CouldNotReadFile)
    }
}

impl VoxChunk {
    pub fn name(&self) -> &str {
        &self.tag_name
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub fn children(&self) -> &Vec<VoxChunk> {
        &self.children
    }

    pub fn find_child(&self, tag_name: &str) -> Option<&VoxChunk> {
        self.children.iter().find(|c| c.tag_name == tag_name)
    }
}

impl SizeChunk {
    pub fn name(&self) -> &str {
        &self.tag_name
    }

    pub fn x(&self) -> u32 {
        self.x_size
    }

    pub fn y(&self) -> u32 {
        self.y_size
    }

    pub fn z(&self) -> u32 {
        self.z_size
    }
}

impl XYZIChunk {
    pub fn voxels(&self) -> &Vec<XYZIVoxel> {
        &self.voxels
    }

    pub fn get_voxels(self) -> Vec<XYZIVoxel> {
        self.voxels
    }
}

impl RGBAChunk {
    pub fn palette(&self) -> &[u32; 256] {
        &self.rgba_palette
    }

    pub fn get_palette(self) -> [u32; 256] {
        self.rgba_palette
    }
}

impl XYZIVoxel {
    pub fn x(&self) -> u8 {
        self.x
    }
    pub fn y(&self) -> u8 {
        self.y
    }
    pub fn z(&self) -> u8 {
        self.z
    }
    pub fn i(&self) -> u8 {
        self.i
    }
}

impl TryInto<SizeChunk> for &VoxChunk {
    type Error = TransformChunkError;
    fn try_into(self) -> Result<SizeChunk, Self::Error> {
        if self.tag_name != "SIZE" {
            return Err(TransformChunkError::WrongTagName);
        }
        if self.children.len() != 0 {
            return Err(TransformChunkError::UnexpectedChildren);
        }
        let number_of_ints = 3;
        let bytes_per_int = 4;
        let expected_size = number_of_ints * bytes_per_int;
        if self.content.len() != expected_size {
            return Err(TransformChunkError::UnexpectedContentSize);
        }
        let x_size = u32::from_le_bytes(self.content[0..4].try_into().unwrap());
        let y_size = u32::from_le_bytes(self.content[4..8].try_into().unwrap());
        let z_size = u32::from_le_bytes(self.content[8..12].try_into().unwrap());

        return Ok(SizeChunk {
            tag_name: self.tag_name.clone(),
            x_size,
            y_size,
            z_size,
        });
    }
}

impl TryInto<XYZIChunk> for &VoxChunk {
    type Error = TransformChunkError;
    fn try_into(self) -> Result<XYZIChunk, Self::Error> {
        if self.tag_name != "XYZI" {
            return Err(TransformChunkError::WrongTagName);
        }

        // I do not support children yet
        if self.children.len() != 0 {
            return Err(TransformChunkError::UnexpectedChildren);
        }

        if self.content.len() < 4 {
            return Err(TransformChunkError::UnexpectedContentSize);
        }

        let number_of_voxels = u32::from_le_bytes(self.content[0..4].try_into().unwrap()) as usize;
        let bytes_per_voxel = 4;

        if self.content.len() - 4 != number_of_voxels * bytes_per_voxel {
            return Err(TransformChunkError::UnexpectedContentSize);
        }

        let voxel_content = &self.content[4..];

        let voxels = voxel_content
            .chunks_exact(bytes_per_voxel)
            .map(|bytes| {
                let x = bytes[0];
                let y = bytes[1];
                let z = bytes[2];
                let i = bytes[3];
                XYZIVoxel { x, y, z, i }
            })
            .collect();

        return Ok(XYZIChunk {
            tag_name: self.tag_name.clone(),
            voxels,
        });
    }
}

impl TryInto<RGBAChunk> for &VoxChunk {
    type Error = TransformChunkError;

    fn try_into(self) -> Result<RGBAChunk, Self::Error> {
        if self.tag_name != "RGBA" {
            return Err(TransformChunkError::WrongTagName);
        }

        if self.children.len() != 0 {
            return Err(TransformChunkError::UnexpectedChildren);
        }

        let color_entries = 256;
        let bytes_per_color = 4;

        if self.content.len() != color_entries * bytes_per_color {
            return Err(TransformChunkError::UnexpectedContentSize);
        }

        // we know we have the exact amount of bytes so its okay to unwrap
        let rgba_palette: Vec<u32> = self
            .content
            .chunks_exact(bytes_per_color)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();

        Ok(RGBAChunk {
            rgba_palette: rgba_palette.try_into().unwrap(),
        })
    }
}

impl TryInto<PackChunk> for &VoxChunk {
    type Error = TransformChunkError;
    fn try_into(self) -> Result<PackChunk, Self::Error> {
        if self.tag_name != "PACK" {
            return Err(TransformChunkError::WrongTagName);
        }

        if self.children.len() != 0 {
            return Err(TransformChunkError::UnexpectedChildren);
        }

        if self.content.len() != 4 {
            return Err(TransformChunkError::UnexpectedContentSize);
        }

        let number_of_models = u32::from_le_bytes(self.content[..].try_into().unwrap());
        Ok(PackChunk { number_of_models })
    }
}

impl Into<VoxChunk> for RGBAChunk {
    fn into(self) -> VoxChunk {
        let content = self
            .rgba_palette
            .iter()
            .flat_map(|color| {
                // tranform a u32 into 4 bytes
                [
                    (color & 0xFF) as u8,
                    ((color & 0xFF00) >> 8) as u8,
                    ((color & 0xFF0000) >> 16) as u8,
                    ((color & 0xFF000000) >> 24) as u8,
                ]
            })
            .collect();
        VoxChunk {
            tag_name: "RGBA".to_owned(),
            content,
            children: vec![],
        }
    }
}

const DEFAULT_PALETTE: [u32; 256] = [
    0x00000000, 0xffffffff, 0xffccffff, 0xff99ffff, 0xff66ffff, 0xff33ffff, 0xff00ffff, 0xffffccff,
    0xffccccff, 0xff99ccff, 0xff66ccff, 0xff33ccff, 0xff00ccff, 0xffff99ff, 0xffcc99ff, 0xff9999ff,
    0xff6699ff, 0xff3399ff, 0xff0099ff, 0xffff66ff, 0xffcc66ff, 0xff9966ff, 0xff6666ff, 0xff3366ff,
    0xff0066ff, 0xffff33ff, 0xffcc33ff, 0xff9933ff, 0xff6633ff, 0xff3333ff, 0xff0033ff, 0xffff00ff,
    0xffcc00ff, 0xff9900ff, 0xff6600ff, 0xff3300ff, 0xff0000ff, 0xffffffcc, 0xffccffcc, 0xff99ffcc,
    0xff66ffcc, 0xff33ffcc, 0xff00ffcc, 0xffffcccc, 0xffcccccc, 0xff99cccc, 0xff66cccc, 0xff33cccc,
    0xff00cccc, 0xffff99cc, 0xffcc99cc, 0xff9999cc, 0xff6699cc, 0xff3399cc, 0xff0099cc, 0xffff66cc,
    0xffcc66cc, 0xff9966cc, 0xff6666cc, 0xff3366cc, 0xff0066cc, 0xffff33cc, 0xffcc33cc, 0xff9933cc,
    0xff6633cc, 0xff3333cc, 0xff0033cc, 0xffff00cc, 0xffcc00cc, 0xff9900cc, 0xff6600cc, 0xff3300cc,
    0xff0000cc, 0xffffff99, 0xffccff99, 0xff99ff99, 0xff66ff99, 0xff33ff99, 0xff00ff99, 0xffffcc99,
    0xffcccc99, 0xff99cc99, 0xff66cc99, 0xff33cc99, 0xff00cc99, 0xffff9999, 0xffcc9999, 0xff999999,
    0xff669999, 0xff339999, 0xff009999, 0xffff6699, 0xffcc6699, 0xff996699, 0xff666699, 0xff336699,
    0xff006699, 0xffff3399, 0xffcc3399, 0xff993399, 0xff663399, 0xff333399, 0xff003399, 0xffff0099,
    0xffcc0099, 0xff990099, 0xff660099, 0xff330099, 0xff000099, 0xffffff66, 0xffccff66, 0xff99ff66,
    0xff66ff66, 0xff33ff66, 0xff00ff66, 0xffffcc66, 0xffcccc66, 0xff99cc66, 0xff66cc66, 0xff33cc66,
    0xff00cc66, 0xffff9966, 0xffcc9966, 0xff999966, 0xff669966, 0xff339966, 0xff009966, 0xffff6666,
    0xffcc6666, 0xff996666, 0xff666666, 0xff336666, 0xff006666, 0xffff3366, 0xffcc3366, 0xff993366,
    0xff663366, 0xff333366, 0xff003366, 0xffff0066, 0xffcc0066, 0xff990066, 0xff660066, 0xff330066,
    0xff000066, 0xffffff33, 0xffccff33, 0xff99ff33, 0xff66ff33, 0xff33ff33, 0xff00ff33, 0xffffcc33,
    0xffcccc33, 0xff99cc33, 0xff66cc33, 0xff33cc33, 0xff00cc33, 0xffff9933, 0xffcc9933, 0xff999933,
    0xff669933, 0xff339933, 0xff009933, 0xffff6633, 0xffcc6633, 0xff996633, 0xff666633, 0xff336633,
    0xff006633, 0xffff3333, 0xffcc3333, 0xff993333, 0xff663333, 0xff333333, 0xff003333, 0xffff0033,
    0xffcc0033, 0xff990033, 0xff660033, 0xff330033, 0xff000033, 0xffffff00, 0xffccff00, 0xff99ff00,
    0xff66ff00, 0xff33ff00, 0xff00ff00, 0xffffcc00, 0xffcccc00, 0xff99cc00, 0xff66cc00, 0xff33cc00,
    0xff00cc00, 0xffff9900, 0xffcc9900, 0xff999900, 0xff669900, 0xff339900, 0xff009900, 0xffff6600,
    0xffcc6600, 0xff996600, 0xff666600, 0xff336600, 0xff006600, 0xffff3300, 0xffcc3300, 0xff993300,
    0xff663300, 0xff333300, 0xff003300, 0xffff0000, 0xffcc0000, 0xff990000, 0xff660000, 0xff330000,
    0xff0000ee, 0xff0000dd, 0xff0000bb, 0xff0000aa, 0xff000088, 0xff000077, 0xff000055, 0xff000044,
    0xff000022, 0xff000011, 0xff00ee00, 0xff00dd00, 0xff00bb00, 0xff00aa00, 0xff008800, 0xff007700,
    0xff005500, 0xff004400, 0xff002200, 0xff001100, 0xffee0000, 0xffdd0000, 0xffbb0000, 0xffaa0000,
    0xff880000, 0xff770000, 0xff550000, 0xff440000, 0xff220000, 0xff110000, 0xffeeeeee, 0xffdddddd,
    0xffbbbbbb, 0xffaaaaaa, 0xff888888, 0xff777777, 0xff555555, 0xff444444, 0xff222222, 0xff111111,
];
