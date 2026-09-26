use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::{E_FAIL, E_POINTER, HMODULE};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::core::Interface;

pub struct D3d11Device {
    native_device: ID3D11Device,
    immediate_context: ID3D11DeviceContext,
    multithread: ID3D10Multithread,
    winrt_device: IDirect3DDevice,
    feature_level: D3D_FEATURE_LEVEL,
}

impl D3d11Device {
    pub fn create_hardware() -> windows::core::Result<Self> {
        let mut native_device = None;
        let mut immediate_context = None;
        let mut feature_level = D3D_FEATURE_LEVEL::default();

        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&raw mut native_device),
                Some(&raw mut feature_level),
                Some(&raw mut immediate_context),
            )?;
        }

        let native_device =
            native_device.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
        let immediate_context =
            immediate_context.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
        let multithread: ID3D10Multithread = native_device.cast()?;
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }
        if !unsafe { multithread.GetMultithreadProtected() }.as_bool() {
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        let dxgi_device: IDXGIDevice = native_device.cast()?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
        let winrt_device: IDirect3DDevice = inspectable.cast()?;

        Ok(Self {
            native_device,
            immediate_context,
            multithread,
            winrt_device,
            feature_level,
        })
    }

    pub const fn feature_level(&self) -> D3D_FEATURE_LEVEL {
        self.feature_level
    }

    pub const fn native_device(&self) -> &ID3D11Device {
        &self.native_device
    }

    pub const fn immediate_context(&self) -> &ID3D11DeviceContext {
        &self.immediate_context
    }

    pub fn multithread_protected(&self) -> bool {
        unsafe { self.multithread.GetMultithreadProtected() }.as_bool()
    }

    pub const fn winrt_device(&self) -> &IDirect3DDevice {
        &self.winrt_device
    }
}
