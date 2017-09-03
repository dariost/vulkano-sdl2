// Copyright (c) 2016 The vulkano developers
// Copyright (c) 2017 Dario Ostuni <dario.ostuni@gmail.com>
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shader_derive;
extern crate sdl2;
extern crate vulkano_sdl2;

use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::Keycode;
use std::mem;
use std::sync::Arc;
use std::thread;

use vulkano::buffer::BufferUsage;
use vulkano::buffer::CpuAccessibleBuffer;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::command_buffer::DynamicState;
use vulkano::device::Device;
use vulkano::framebuffer::Framebuffer;
use vulkano::framebuffer::Subpass;
use vulkano::instance::Instance;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::pipeline::viewport::Viewport;
use vulkano::swapchain;
use vulkano::swapchain::AcquireError;
use vulkano::swapchain::PresentMode;
use vulkano::swapchain::SurfaceTransform;
use vulkano::swapchain::Swapchain;
use vulkano::swapchain::SwapchainCreationError;
use vulkano::sync::GpuFuture;
use vulkano::sync::now;

fn main()
{
    let sdl = sdl2::init().unwrap();
    let sdl_video = sdl.video().unwrap();
    let window = sdl_video
        .window("Vulkan Triangle", 800, 600)
        .position_centered()
        .resizable()
        .allow_highdpi()
        .build()
        .unwrap();
    let instance = {
        let extensions = vulkano_sdl2::required_extensions(&window).unwrap();
        Instance::new(None, &extensions, None).expect("failed to create Vulkan instance")
    };
    let physical = vulkano::instance::PhysicalDevice::enumerate(&instance)
        .next()
        .expect("no device available");
    println!("Using device: {} (type: {:?})", physical.name(), physical.ty());
    let surface = vulkano_sdl2::build_vk_surface(&window, instance.clone()).unwrap();
    let mut dimensions = {
        let (width, height) = window.size();
        [width, height]
    };
    let queue = physical
        .queue_families()
        .find(|&q| q.supports_graphics() && surface.is_supported(q).unwrap_or(false))
        .expect("couldn't find a graphical queue family");
    let (device, mut queues) = {
        let device_ext = vulkano::device::DeviceExtensions {
            khr_swapchain: true,
            ..vulkano::device::DeviceExtensions::none()
        };

        Device::new(physical, physical.supported_features(), &device_ext, [(queue, 0.5)].iter().cloned())
            .expect("failed to create device")
    };
    let queue = queues.next().unwrap();
    let (mut swapchain, mut images) = {
        let caps = surface.capabilities(physical).expect(
            "failed to get surface capabilities",
        );
        let present_mode = {
            if caps.present_modes.supports(PresentMode::Mailbox)
            {
                PresentMode::Mailbox
            }
            else if caps.present_modes.supports(PresentMode::Immediate)
            {
                PresentMode::Immediate
            }
            else if caps.present_modes.supports(PresentMode::Relaxed)
            {
                PresentMode::Relaxed
            }
            else
            {
                PresentMode::Fifo
            }
        };
        let alpha = caps.supported_composite_alpha.iter().next().unwrap();
        let format = caps.supported_formats[0].0;
        Swapchain::new(
            device.clone(),
            surface.clone(),
            caps.min_image_count,
            format,
            dimensions,
            1,
            caps.supported_usage_flags,
            &queue,
            SurfaceTransform::Identity,
            alpha,
            present_mode,
            true,
            None,
        ).expect("failed to create swapchain")
    };
    let vertex_buffer = {
        #[derive(Debug, Clone)]
        struct Vertex
        {
            position: [f32; 2],
            color: [f32; 3],
        }
        impl_vertex!(Vertex, position, color);

        CpuAccessibleBuffer::from_iter(
            device.clone(),
            BufferUsage::all(),
            [
                Vertex {
                    position: [-0.5, 0.5],
                    color: [1.0, 0.0, 0.0],
                },
                Vertex {
                    position: [0.5, 0.5],
                    color: [0.0, 1.0, 0.0],
                },
                Vertex {
                    position: [0.0, -0.5],
                    color: [0.0, 0.0, 1.0],
                },
            ].iter()
                .cloned(),
        ).expect("failed to create buffer")
    };
    #[allow(dead_code)]
    mod vs {
        #[derive(VulkanoShader)]
        #[ty = "vertex"]
        #[src = "
#version 450

layout(location = 0) in vec2 position;
layout(location = 1) in vec3 color;

layout(location = 0) out vec3 fragment_color;

void main() {
    fragment_color = color;
    gl_Position = vec4(position, 0.0, 1.0);
}
"]
        struct Dummy;
    }
    #[allow(dead_code)]
    mod fs {
        #[derive(VulkanoShader)]
        #[ty = "fragment"]
        #[src = "
#version 450

layout(location = 0) in vec3 fragment_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = vec4(fragment_color, 1.0);
}
"]
        struct Dummy;
    }
    let vs = vs::Shader::load(device.clone()).expect("failed to create shader module");
    let fs = fs::Shader::load(device.clone()).expect("failed to create shader module");
    let render_pass = Arc::new(
        single_pass_renderpass!(device.clone(),
        attachments: {
            color: {
                load: Clear,
                store: Store,
                format: swapchain.format(),
                samples: 1,
            }
        },
        pass: {
            color: [color],
            depth_stencil: {}
        }
    ).unwrap(),
    );
    let pipeline = Arc::new(
        GraphicsPipeline::start()
            .vertex_input_single_buffer()
            .vertex_shader(vs.main_entry_point(), ())
            .triangle_list()
            .viewports_dynamic_scissors_irrelevant(1)
            .fragment_shader(fs.main_entry_point(), ())
            .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
            .build(device.clone())
            .unwrap(),
    );
    let mut framebuffers: Option<Vec<Arc<vulkano::framebuffer::Framebuffer<_, _>>>> = None;
    let mut recreate_swapchain = false;
    let mut previous_frame_end = Box::new(now(device.clone())) as Box<GpuFuture>;
    let mut event_pump = sdl.event_pump().unwrap();
    loop
    {
        previous_frame_end.cleanup_finished();
        if recreate_swapchain
        {
            dimensions = {
                let (new_width, new_height) = window.size();
                [new_width, new_height]
            };
            let (new_swapchain, new_images) = match swapchain.recreate_with_dimension(dimensions)
            {
                Ok(r) => r,
                Err(SwapchainCreationError::UnsupportedDimensions) =>
                {
                    continue;
                }
                Err(err) => panic!("{:?}", err),
            };
            mem::replace(&mut swapchain, new_swapchain);
            mem::replace(&mut images, new_images);
            framebuffers = None;
            recreate_swapchain = false;
        }
        if framebuffers.is_none()
        {
            let new_framebuffers = Some(
                images
                    .iter()
                    .map(|image| {
                        Arc::new(
                            Framebuffer::start(render_pass.clone())
                                .add(image.clone())
                                .unwrap()
                                .build()
                                .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            mem::replace(&mut framebuffers, new_framebuffers);
        }
        let (image_num, acquire_future) = match swapchain::acquire_next_image(swapchain.clone(), None)
        {
            Ok(r) => r,
            Err(AcquireError::OutOfDate) =>
            {
                recreate_swapchain = true;
                continue;
            }
            Err(err) => panic!("{:?}", err),
        };
        let command_buffer = AutoCommandBufferBuilder::primary_one_time_submit(device.clone(), queue.family())
            .unwrap()
            .begin_render_pass(framebuffers.as_ref().unwrap()[image_num].clone(), false, vec![[0.0, 0.0, 0.0, 1.0].into()])
            .unwrap()
            .draw(
                pipeline.clone(),
                DynamicState {
                    line_width: None,
                    viewports: Some(vec![
                        Viewport {
                            origin: [0.0, 0.0],
                            dimensions: [dimensions[0] as f32, dimensions[1] as f32],
                            depth_range: 0.0..1.0,
                        },
                    ]),
                    scissors: None,
                },
                vertex_buffer.clone(),
                (),
                (),
            )
            .unwrap()
            .end_render_pass()
            .unwrap()
            .build()
            .unwrap();
        let future = previous_frame_end
            .join(acquire_future)
            .then_execute(queue.clone(), command_buffer)
            .unwrap()
            .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
            .then_signal_fence_and_flush()
            .unwrap();
        previous_frame_end = Box::new(future) as Box<_>;
        let mut done = false;
        for event in event_pump.poll_iter()
        {
            match event
            {
                Event::Quit { .. } |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => done = true,
                Event::Window { win_event: WindowEvent::SizeChanged(_, _), .. } => recreate_swapchain = true,
                _ =>
                {}
            }
        }
        if done
        {
            return;
        }
        thread::yield_now();
    }
}
