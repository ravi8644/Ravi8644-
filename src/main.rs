// Cinnabar -> Xbox: prova de conceito.
// Abre uma janela UWP (CoreWindow) e pinta a tela com uma cor sólida:
//   build padrão (D3D11 puro)     -> tela VERDE
//   build --features wgpu-render  -> tela AZUL
// Tela preta = o app abriu mas o render falhou. Fechou sozinho = falha ao iniciar.
// O diagnóstico vai para LocalState\poc.log (Device Portal > File Explorer).

#![windows_subsystem = "windows"]

use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::Duration,
};

use windows::{
    core::{implement, Ref, Result, HSTRING},
    ApplicationModel::Core::{
        CoreApplication, CoreApplicationView, IFrameworkView, IFrameworkViewSource,
        IFrameworkViewSource_Impl, IFrameworkView_Impl,
    },
    Foundation::TypedEventHandler,
    Storage::ApplicationData,
    UI::Core::{CoreProcessEventsOption, CoreWindow, CoreWindowEventArgs},
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

static LOG_PATH: OnceLock<String> = OnceLock::new();
static CLOSED: AtomicBool = AtomicBool::new(false);

fn log(msg: &str) {
    let path = LOG_PATH.get_or_init(|| {
        ApplicationData::Current()
            .and_then(|d| d.LocalFolder())
            .and_then(|f| f.Path())
            .map(|p| format!("{}\\poc.log", p))
            .unwrap_or_else(|_| "poc.log".to_string())
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}

#[implement(IFrameworkViewSource)]
struct PocSource;

impl IFrameworkViewSource_Impl for PocSource_Impl {
    fn CreateView(&self) -> Result<IFrameworkView> {
        Ok(PocView.into())
    }
}

#[implement(IFrameworkView)]
struct PocView;

impl IFrameworkView_Impl for PocView_Impl {
    fn Initialize(&self, _: Ref<'_, CoreApplicationView>) -> Result<()> {
        Ok(())
    }
    fn SetWindow(&self, _: Ref<'_, CoreWindow>) -> Result<()> {
        Ok(())
    }
    fn Load(&self, _: &HSTRING) -> Result<()> {
        Ok(())
    }
    fn Run(&self) -> Result<()> {
        run()
    }
    fn Uninitialize(&self) -> Result<()> {
        Ok(())
    }
}

fn run() -> Result<()> {
    log("Run: inicio");
    let window = CoreWindow::GetForCurrentThread()?;
    window.Closed(&TypedEventHandler::<CoreWindow, CoreWindowEventArgs>::new(|_, _| {
        CLOSED.store(true, Ordering::SeqCst);
        Ok(())
    }))?;
    window.Activate()?;
    let dispatcher = window.Dispatcher()?;

    let bounds = window.Bounds()?;
    let (w, h) = ((bounds.Width as u32).max(1), (bounds.Height as u32).max(1));
    log(&format!("janela: {w}x{h}"));

    let mut renderer = match gfx::Renderer::new(&window, w, h) {
        Ok(r) => {
            log("render: iniciado");
            Some(r)
        }
        Err(e) => {
            log(&format!("render: FALHOU ao iniciar: {e}"));
            None
        }
    };

    let mut frames: u64 = 0;
    while !CLOSED.load(Ordering::SeqCst) {
        dispatcher.ProcessEvents(CoreProcessEventsOption::ProcessAllIfPresent)?;
        let failed = match renderer.as_mut() {
            Some(r) => r.render().err(),
            None => {
                std::thread::sleep(Duration::from_millis(50));
                None
            }
        };
        if let Some(e) = failed {
            log(&format!("render: FALHOU no frame {frames}: {e}"));
            renderer = None;
        }
        frames += 1;
        if frames == 1 || frames == 600 {
            log(&format!("frame {frames}"));
        }
    }
    log("Run: fim");
    Ok(())
}

fn main() {
    std::panic::set_hook(Box::new(|info| log(&format!("PANIC: {info}"))));
    if let Err(e) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        log(&format!("RoInitialize(MTA) falhou: {e}"));
        return;
    }
    log("WinRT: MTA inicializado");
    log("main: inicio");
    let source: IFrameworkViewSource = PocSource.into();
    if let Err(e) = CoreApplication::Run(&source) {
        log(&format!("CoreApplication::Run falhou: {e}"));
    }
    unsafe { RoUninitialize() };
}

// ---------------------------------------------------------------- D3D11 (verde)
#[cfg(not(feature = "wgpu-render"))]
mod gfx {
    use windows::{
        core::{Error, Interface, Result},
        Win32::{
            Foundation::{E_FAIL, HMODULE},
            Graphics::{
                Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
                Direct3D11::*,
                Dxgi::{Common::*, *},
            },
        },
        UI::Core::CoreWindow,
    };

    pub struct Renderer {
        device: ID3D11Device,
        ctx: ID3D11DeviceContext,
        swap: IDXGISwapChain1,
    }

    impl Renderer {
        pub fn new(window: &CoreWindow, w: u32, h: u32) -> Result<Self> {
            unsafe {
                let mut device: Option<ID3D11Device> = None;
                let mut ctx: Option<ID3D11DeviceContext> = None;
                D3D11CreateDevice(
                    None::<&IDXGIAdapter>,
                    D3D_DRIVER_TYPE_HARDWARE,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&[D3D_FEATURE_LEVEL_11_0]),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut ctx),
                )?;
                let device = device.ok_or_else(|| Error::from(E_FAIL))?;
                let ctx = ctx.ok_or_else(|| Error::from(E_FAIL))?;

                let dxgi: IDXGIDevice = device.cast()?;
                let adapter = dxgi.GetAdapter()?;
                let factory: IDXGIFactory2 = adapter.GetParent()?;

                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: w,
                    Height: h,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    Stereo: false.into(),
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_NONE,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    Flags: 0,
                };
                let swap = factory.CreateSwapChainForCoreWindow(
                    &device,
                    window,
                    &desc,
                    None::<&IDXGIOutput>,
                )?;
                Ok(Self { device, ctx, swap })
            }
        }

        pub fn render(&mut self) -> Result<()> {
            unsafe {
                let tex: ID3D11Texture2D = self.swap.GetBuffer(0)?;
                let mut rtv: Option<ID3D11RenderTargetView> = None;
                self.device.CreateRenderTargetView(&tex, None, Some(&mut rtv))?;
                let rtv = rtv.ok_or_else(|| Error::from(E_FAIL))?;
                self.ctx.ClearRenderTargetView(&rtv, &[0.0, 0.6, 0.2, 1.0]);
                self.swap.Present(1, DXGI_PRESENT(0)).ok()?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- wgpu (azul)
#[cfg(feature = "wgpu-render")]
mod gfx {
    use std::ptr::NonNull;

    use raw_window_handle::{
        RawDisplayHandle, RawWindowHandle, WinRtWindowHandle, WindowsDisplayHandle,
    };
    use windows::{
        core::{Error, Interface, Result},
        Win32::Foundation::E_FAIL,
        UI::Core::CoreWindow,
    };

    fn fail(msg: String) -> Error {
        crate::log(&msg);
        Error::from(E_FAIL)
    }

    pub struct Renderer {
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    impl Renderer {
        pub fn new(window: &CoreWindow, w: u32, h: u32) -> Result<Self> {
            let instance = wgpu::Instance::default();
            let core_window =
                NonNull::new(window.as_raw()).ok_or_else(|| fail("CoreWindow nulo".into()))?;

            // Ponto de maior risco: o backend DX12 do wgpu aceitar um handle WinRt.
            let surface = unsafe {
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
                    raw_window_handle: RawWindowHandle::WinRt(WinRtWindowHandle::new(core_window)),
                })
            }
            .map_err(|e| fail(format!("wgpu create_surface: {e}")))?;
            crate::log("wgpu: surface criada");

            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .ok_or_else(|| fail("wgpu: nenhum adapter".into()))?;
            crate::log(&format!("wgpu: adapter {:?}", adapter.get_info()));

            let (device, queue) = pollster::block_on(
                adapter.request_device(&wgpu::DeviceDescriptor::default(), None),
            )
            .map_err(|e| fail(format!("wgpu request_device: {e}")))?;

            let caps = surface.get_capabilities(&adapter);
            let format = caps
                .formats
                .first()
                .copied()
                .ok_or_else(|| fail("wgpu: surface sem formatos".into()))?;
            let alpha_mode = caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto);
            surface.configure(
                &device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format,
                    width: w,
                    height: h,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode,
                    view_formats: vec![],
                },
            );
            Ok(Self { surface, device, queue })
        }

        pub fn render(&mut self) -> Result<()> {
            let frame = self
                .surface
                .get_current_texture()
                .map_err(|e| fail(format!("wgpu get_current_texture: {e}")))?;
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.1,
                                g: 0.3,
                                b: 0.9,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            self.queue.submit(Some(enc.finish()));
            frame.present();
            Ok(())
        }
    }
}
