use glam::{Mat3, Vec3};

// In NDC coordinates
// The Y axis is down the screen, the X axis is to the right and the Z axis in inwards the screen

#[derive(Clone, Copy, Default, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub direction: Vec3,
    pub up: Vec3,
    pub focal_distance: f32,
    pub aperture: f32,
}

impl Camera {
    pub fn new_at(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: Vec3::new(x, y, z),
            direction: (Vec3::ZERO - Vec3::new(x, y, z)).normalize(),
            up: Vec3::new(0.0, -1.0, 0.0),
            focal_distance: 30.0,
            aperture: 0.0,
        }
    }

    pub fn rotate_vertically(&mut self, amount_radians: f32) {
        let right = self.direction.cross(self.up).normalize();
        let rotation_matrix = Mat3::from_axis_angle(right, amount_radians);
        let new_direction = rotation_matrix * self.direction;
        if new_direction.y > 0.9 || new_direction.y < -0.9 {
            return;
        }
        self.direction = rotation_matrix * self.direction;
    }

    pub fn rotate_horizontally(&mut self, amount_radians: f32) {
        let rotation_matrix = Mat3::from_axis_angle(self.up, amount_radians);
        self.direction = rotation_matrix * self.direction;
    }

    pub fn move_forward(&mut self, amount: f32) {
        self.position += self.direction * amount;
    }

    pub fn move_right(&mut self, amount: f32) {
        let right = self.direction.cross(self.up).normalize();
        self.position += right * amount;
    }

    pub fn move_up(&mut self, amount: f32) {
        self.position += self.up * amount;
    }

    pub fn move_spherically_while_looking_at(
        &mut self,
        point: Vec3,
        vertical_amount: f32,
        horizontal_amount: f32,
    ) {
        let mut direction = point - self.position;
        let distance = direction.length();
        direction = direction.normalize();
        let mut horizontal_angle = (direction.x * direction.x + direction.z * direction.z)
            .sqrt()
            .atan2(-direction.y);
        let mut vertical_angle = direction.z.atan2(direction.x);
        horizontal_angle += horizontal_amount;
        vertical_angle += vertical_amount;
        vertical_angle = vertical_angle.clamp(
            -std::f32::consts::FRAC_PI_2 - 0.1,
            std::f32::consts::FRAC_PI_2 - 0.1,
        );

        self.position = point
            - Vec3::new(
                horizontal_angle.sin() * vertical_angle.cos(),
                -vertical_angle.cos(),
                horizontal_angle.sin() * vertical_angle.sin(),
            ) * distance;

        self.direction = direction;
    }
}
