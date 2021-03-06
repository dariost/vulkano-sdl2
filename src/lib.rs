extern crate sdl2;
extern crate vulkano;

use sdl2::sys::{SDL_GetError, SDL_GetWindowWMInfo, SDL_SysWMinfo};
use sdl2::sys::{SDL_MAJOR_VERSION, SDL_MINOR_VERSION, SDL_PATCHLEVEL};
use sdl2::sys::SDL_SYSWM_TYPE;
use sdl2::sys::SDL_Window;
use sdl2::sys::SDL_bool::SDL_FALSE;
use sdl2::video::Window;
use std::ffi::CString;
use std::mem;
use std::os::raw::c_char;
use std::sync::Arc;
use vulkano::instance::{Instance, InstanceExtensions};
use vulkano::swapchain::{Surface, SurfaceCreationError};

#[derive(Debug)]
pub enum ErrorType
{
    Unknown,
    PlatformNotSupported,
    OutOfMemory,
    MissingExtension(String),
    Generic(String),
}

pub fn required_extensions(window: &Window) -> Result<InstanceExtensions, ErrorType>
{
    let wm_info = get_wminfo(window.raw())?;
    let mut extensions = InstanceExtensions {
        khr_surface: true,
        ..InstanceExtensions::none()
    };
    match wm_info.subsystem
    {
        SDL_SYSWM_TYPE::SDL_SYSWM_X11 => extensions.khr_xlib_surface = true,
        SDL_SYSWM_TYPE::SDL_SYSWM_WAYLAND => extensions.khr_wayland_surface = true,
        SDL_SYSWM_TYPE::SDL_SYSWM_WINDOWS => extensions.khr_win32_surface = true,
        SDL_SYSWM_TYPE::SDL_SYSWM_ANDROID => extensions.khr_android_surface = true,
        _ => return Err(ErrorType::PlatformNotSupported),
    }
    Ok(extensions)
}

pub fn build_vk_surface(window: &Window, instance: Arc<Instance>) -> Result<Arc<Surface>, ErrorType>
{
    let wm_info = get_wminfo(window.raw())?;
    unsafe { sdl2_to_surface(&wm_info, instance) }
}

#[cfg(target_os = "android")]
unsafe fn sdl2_to_surface(wm_info: &SDL_SysWMinfo, instance: Arc<Instance>) -> Result<Arc<Surface>, ErrorType>
{
    let window = wm_info.info.android.window;
    translate_vk_result(Surface::from_anativewindow(instance, window))
}

#[cfg(all(unix, not(target_os = "android")))]
unsafe fn sdl2_to_surface(wm_info: &SDL_SysWMinfo, instance: Arc<Instance>) -> Result<Arc<Surface>, ErrorType>
{
    if wm_info.subsystem == SDL_SYSWM_TYPE::SDL_SYSWM_X11
    {
        let display = wm_info.info.x11.display;
        let window = wm_info.info.x11.window;
        translate_vk_result(Surface::from_xlib(instance, display, window))
    }
    else if wm_info.subsystem == SDL_SYSWM_TYPE::SDL_SYSWM_WAYLAND
    {
        let display = wm_info.info.wl.display;
        let surface = wm_info.info.wl.surface;
        translate_vk_result(Surface::from_wayland(instance, display, surface))
    }
    else
    {
        unreachable!();
    }
}

#[cfg(target_os = "windows")]
unsafe fn sdl2_to_surface(wm_info: &SDL_SysWMinfo, instance: Arc<Instance>) -> Result<Arc<Surface>, ErrorType>
{
    let hinstance = wm_info.info.win.hinstance;
    let hwnd = wm_info.info.win.window;
    translate_vk_result(Surface::from_hwnd(instance, hinstance, hwnd))
}

fn translate_vk_result(obj: Result<Arc<Surface>, SurfaceCreationError>) -> Result<Arc<Surface>, ErrorType>
{
    match obj
    {
        Ok(x) => Ok(x),
        Err(SurfaceCreationError::OomError(_)) => Err(ErrorType::OutOfMemory),
        Err(SurfaceCreationError::MissingExtension { name: x }) => Err(ErrorType::MissingExtension(String::from(x))),
    }
}

fn get_wminfo(window: *mut SDL_Window) -> Result<SDL_SysWMinfo, ErrorType>
{
    let mut wm_info: SDL_SysWMinfo;
    unsafe {
        wm_info = mem::zeroed();
    }
    wm_info.version.major = SDL_MAJOR_VERSION as u8;
    wm_info.version.minor = SDL_MINOR_VERSION as u8;
    wm_info.version.patch = SDL_PATCHLEVEL as u8;
    unsafe {
        if SDL_GetWindowWMInfo(window, &mut wm_info as *mut SDL_SysWMinfo) == SDL_FALSE
        {
            let error = CString::from_raw(SDL_GetError() as *mut c_char);
            match error.into_string()
            {
                Ok(x) => return Err(ErrorType::Generic(x)),
                Err(_) => return Err(ErrorType::Unknown),
            }
        }
    }
    Ok(wm_info)
}
