use std::sync::Arc;

use vulkano::device::physical::PhysicalDevice;
use vulkano::device::{
    Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
};
use vulkano::format::Format;
use vulkano::image::{Image, ImageUsage};
use vulkano::instance::{Instance, InstanceCreateFlags, InstanceCreateInfo, InstanceExtensions};
use vulkano::swapchain::{Surface, Swapchain, SwapchainCreateInfo};
use vulkano::{Validated, VulkanLibrary};
use winit::dpi::PhysicalSize;
use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use winit::window::Window;

fn get_device_extentions() -> DeviceExtensions {
    DeviceExtensions {
        khr_swapchain: true,
        ..Default::default()
    }
}

pub fn get_instance(window: &Arc<Window>) -> Arc<Instance> {
    let mut extensions = InstanceExtensions {
        khr_surface: true,
        ..InstanceExtensions::empty()
    };
    match window.display_handle().unwrap().as_raw() {
        RawDisplayHandle::Android(_) => extensions.khr_android_surface = true,
        RawDisplayHandle::AppKit(_) => extensions.ext_metal_surface = true,
        RawDisplayHandle::UiKit(_) => extensions.ext_metal_surface = true,
        RawDisplayHandle::Windows(_) => extensions.khr_win32_surface = true,
        RawDisplayHandle::Wayland(_) => extensions.khr_wayland_surface = true,
        RawDisplayHandle::Xcb(_) => extensions.khr_xcb_surface = true,
        RawDisplayHandle::Xlib(_) => extensions.khr_xlib_surface = true,
        _ => unimplemented!(),
    };

    let library = VulkanLibrary::new().expect("no local Vulkan library/DLL");
    Instance::new(
        library,
        InstanceCreateInfo {
            flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            enabled_extensions: extensions,
            ..Default::default()
        },
    )
    .expect("failed to create instance")
}

pub fn get_swapchain(
    device: Arc<Device>,
    physical_device: &Arc<PhysicalDevice>,
    surface: Arc<Surface>,
    dimensions: PhysicalSize<u32>,
) -> (Arc<Swapchain>, Vec<Arc<Image>>) {
    let max_push_constant_size = physical_device.properties().max_push_constants_size;
    println!("Max size of push constants: {max_push_constant_size}mb");
    let caps = physical_device
        .surface_capabilities(&surface, Default::default())
        .unwrap();
    let composite_alpha = caps.supported_composite_alpha.into_iter().next().unwrap();
    let wanted_format = Format::R8G8B8A8_UNORM;
    physical_device
        .surface_formats(&surface, Default::default())
        .unwrap()
        .iter()
        .position(|(format, _)| *format == wanted_format)
        .expect("Couldn't find the format desired: {wanted_format}");
    Swapchain::new(
        device,
        surface,
        SwapchainCreateInfo {
            min_image_count: caps.min_image_count, // How many buffers to use in the swapchain
            image_format: wanted_format,
            image_extent: dimensions.into(),
            image_usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_DST, // What the images are going to be used for
            composite_alpha,
            ..Default::default()
        },
    )
    .map_err(Validated::unwrap)
    .unwrap()
}

pub fn get_physical_device_and_family_index(
    surface: &Arc<Surface>,
    instance: &Arc<Instance>,
) -> (Arc<PhysicalDevice>, u32) {
    instance
        .enumerate_physical_devices()
        .unwrap()
        .filter(|d| d.supported_extensions().contains(&get_device_extentions()))
        .filter_map(|d| {
            d.queue_family_properties()
                .iter()
                .enumerate()
                .position(|(i, queue)| {
                    let flags = &queue.queue_flags;
                    flags.contains(QueueFlags::GRAPHICS)
                        && flags.contains(QueueFlags::COMPUTE)
                        && d.surface_support(i as u32, surface).unwrap_or(false)
                })
                .map(|p| (d, p as u32))
        })
        .min_by_key(|(p, _)| match p.properties().device_type {
            vulkano::device::physical::PhysicalDeviceType::DiscreteGpu => 1,
            vulkano::device::physical::PhysicalDeviceType::IntegratedGpu => 2,
            vulkano::device::physical::PhysicalDeviceType::VirtualGpu => 3,
            vulkano::device::physical::PhysicalDeviceType::Cpu => 4,
            _ => 4,
        })
        .unwrap()
}

pub fn get_device_and_queue(
    physical_device: Arc<PhysicalDevice>,
    queue_family_index: u32,
) -> (Arc<Device>, Arc<Queue>) {
    let (device, mut queues) = Device::new(
        physical_device,
        DeviceCreateInfo {
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            enabled_extensions: get_device_extentions(),
            ..Default::default()
        },
    )
    .unwrap();

    let queue = queues.next().unwrap();

    (device, queue)
}
