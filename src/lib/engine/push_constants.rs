// Vec3's get padded to vec 4's anyway. So I instead of leaving those bytes to waste we use them to represent the camera

use glam::Vec3;
use vulkano::buffer::BufferContents;

use crate::camera::Camera;

// we have to use vec4's instead of vec3's because of how the alignment works in push constants in vulkan
// see: https://doc.rust-lang.org/reference/type-layout.html#r-layout.repr.align-packed
#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug)]
pub struct PushConstants {
    top_left_pixel: Vec3,
    camera_x: f32,
    pixel_delta_right: Vec3,
    camera_y: f32,
    pixel_delta_down: Vec3,
    camera_z: f32,
    shader_flags: u8,
}

impl PushConstants {
    pub fn new(image_size: &[u32; 2], camera: &Camera, shader_flags: u8) -> Self {
        let up_vector: Vec3 = camera.up;
        let image_width = image_size[0] as f32;
        let image_height = image_size[1] as f32;
        let aspect_ratio = image_width / image_height;

        let camera_center = camera.position;

        let viewport_height =
            2.0 * camera.focal_distance * (camera.field_of_view.to_radians() / 2.0).tan();
        let viewport_width = aspect_ratio * viewport_height;
        let camera_relative_forward = camera.direction.normalize();

        let camera_relative_right = camera_relative_forward.cross(up_vector).normalize();
        let camera_relative_down = camera_relative_forward.cross(camera_relative_right);

        let viewport_right_vector = viewport_width * camera_relative_right;
        let viewport_down_vector = viewport_height * camera_relative_down;

        let pixel_delta_right = viewport_right_vector / image_width;
        let pixel_delta_down = viewport_down_vector / image_height;

        let viepwort_upper_left = 0.5 * (-viewport_down_vector - viewport_right_vector)
            + camera_center
            + camera_relative_forward * camera.focal_distance;

        let top_left_pixel = viepwort_upper_left + 0.5 * (pixel_delta_down + pixel_delta_right);

        Self {
            top_left_pixel,
            pixel_delta_right,
            pixel_delta_down,
            camera_x: camera_center.x,
            camera_y: camera_center.y,
            camera_z: camera_center.z,
            shader_flags,
        }
    }
}
