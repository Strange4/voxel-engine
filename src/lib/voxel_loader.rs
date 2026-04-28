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

        let main_chunk = Self::read_chunk_recursive(&mut file)?;
        if main_chunk.tag_name != "MAIN" {
            return Err(LoadVoxError::NoMainChunk);
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
        let mut rgba_palette: Vec<u32> = self
            .content
            .chunks_exact(bytes_per_color)
            .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
            .collect();
        // need to remap the palettes: https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox.txt
        rgba_palette.insert(0, 0);
        rgba_palette.pop();
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

/// RGBA instead of ABGR like they give it on the format website
pub const DEFAULT_PALETTE: [u32; 256] = [
    0x00000000, 0xffffffff, 0xffffccff, 0xffff99ff, 0xffff66ff, 0xffff33ff, 0xffff00ff, 0xffccffff,
    0xffccccff, 0xffcc99ff, 0xffcc66ff, 0xffcc33ff, 0xffcc00ff, 0xff99ffff, 0xff99ccff, 0xff9999ff,
    0xff9966ff, 0xff9933ff, 0xff9900ff, 0xff66ffff, 0xff66ccff, 0xff6699ff, 0xff6666ff, 0xff6633ff,
    0xff6600ff, 0xff33ffff, 0xff33ccff, 0xff3399ff, 0xff3366ff, 0xff3333ff, 0xff3300ff, 0xff00ffff,
    0xff00ccff, 0xff0099ff, 0xff0066ff, 0xff0033ff, 0xff0000ff, 0xccffffff, 0xccffccff, 0xccff99ff,
    0xccff66ff, 0xccff33ff, 0xccff00ff, 0xccccffff, 0xccccccff, 0xcccc99ff, 0xcccc66ff, 0xcccc33ff,
    0xcccc00ff, 0xcc99ffff, 0xcc99ccff, 0xcc9999ff, 0xcc9966ff, 0xcc9933ff, 0xcc9900ff, 0xcc66ffff,
    0xcc66ccff, 0xcc6699ff, 0xcc6666ff, 0xcc6633ff, 0xcc6600ff, 0xcc33ffff, 0xcc33ccff, 0xcc3399ff,
    0xcc3366ff, 0xcc3333ff, 0xcc3300ff, 0xcc00ffff, 0xcc00ccff, 0xcc0099ff, 0xcc0066ff, 0xcc0033ff,
    0xcc0000ff, 0x99ffffff, 0x99ffccff, 0x99ff99ff, 0x99ff66ff, 0x99ff33ff, 0x99ff00ff, 0x99ccffff,
    0x99ccccff, 0x99cc99ff, 0x99cc66ff, 0x99cc33ff, 0x99cc00ff, 0x9999ffff, 0x9999ccff, 0x999999ff,
    0x999966ff, 0x999933ff, 0x999900ff, 0x9966ffff, 0x9966ccff, 0x996699ff, 0x996666ff, 0x996633ff,
    0x996600ff, 0x9933ffff, 0x9933ccff, 0x993399ff, 0x993366ff, 0x993333ff, 0x993300ff, 0x9900ffff,
    0x9900ccff, 0x990099ff, 0x990066ff, 0x990033ff, 0x990000ff, 0x66ffffff, 0x66ffccff, 0x66ff99ff,
    0x66ff66ff, 0x66ff33ff, 0x66ff00ff, 0x66ccffff, 0x66ccccff, 0x66cc99ff, 0x66cc66ff, 0x66cc33ff,
    0x66cc00ff, 0x6699ffff, 0x6699ccff, 0x669999ff, 0x669966ff, 0x669933ff, 0x669900ff, 0x6666ffff,
    0x6666ccff, 0x666699ff, 0x666666ff, 0x666633ff, 0x666600ff, 0x6633ffff, 0x6633ccff, 0x663399ff,
    0x663366ff, 0x663333ff, 0x663300ff, 0x6600ffff, 0x6600ccff, 0x660099ff, 0x660066ff, 0x660033ff,
    0x660000ff, 0x33ffffff, 0x33ffccff, 0x33ff99ff, 0x33ff66ff, 0x33ff33ff, 0x33ff00ff, 0x33ccffff,
    0x33ccccff, 0x33cc99ff, 0x33cc66ff, 0x33cc33ff, 0x33cc00ff, 0x3399ffff, 0x3399ccff, 0x339999ff,
    0x339966ff, 0x339933ff, 0x339900ff, 0x3366ffff, 0x3366ccff, 0x336699ff, 0x336666ff, 0x336633ff,
    0x336600ff, 0x3333ffff, 0x3333ccff, 0x333399ff, 0x333366ff, 0x333333ff, 0x333300ff, 0x3300ffff,
    0x3300ccff, 0x330099ff, 0x330066ff, 0x330033ff, 0x330000ff, 0x00ffffff, 0x00ffccff, 0x00ff99ff,
    0x00ff66ff, 0x00ff33ff, 0x00ff00ff, 0x00ccffff, 0x00ccccff, 0x00cc99ff, 0x00cc66ff, 0x00cc33ff,
    0x00cc00ff, 0x0099ffff, 0x0099ccff, 0x009999ff, 0x009966ff, 0x009933ff, 0x009900ff, 0x0066ffff,
    0x0066ccff, 0x006699ff, 0x006666ff, 0x006633ff, 0x006600ff, 0x0033ffff, 0x0033ccff, 0x003399ff,
    0x003366ff, 0x003333ff, 0x003300ff, 0x0000ffff, 0x0000ccff, 0x000099ff, 0x000066ff, 0x000033ff,
    0xee0000ff, 0xdd0000ff, 0xbb0000ff, 0xaa0000ff, 0x880000ff, 0x770000ff, 0x550000ff, 0x440000ff,
    0x220000ff, 0x110000ff, 0x00ee00ff, 0x00dd00ff, 0x00bb00ff, 0x00aa00ff, 0x008800ff, 0x007700ff,
    0x005500ff, 0x004400ff, 0x002200ff, 0x001100ff, 0x0000eeff, 0x0000ddff, 0x0000bbff, 0x0000aaff,
    0x000088ff, 0x000077ff, 0x000055ff, 0x000044ff, 0x000022ff, 0x000011ff, 0xeeeeeeff, 0xddddddff,
    0xbbbbbbff, 0xaaaaaaff, 0x888888ff, 0x777777ff, 0x555555ff, 0x444444ff, 0x222222ff, 0x111111ff,
];
