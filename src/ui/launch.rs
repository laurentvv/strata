//! How the window is drawn. OpenGL is tried first (light and fast); when it cannot be set up (virtual
//! machines such as Hyper-V, remote sessions, very old drivers) the window is drawn through DirectX 12
//! instead. DirectX has a software adapter (WARP) that runs on the CPU, so Strata opens even on a machine
//! with no graphics driver at all.

use eframe::egui_wgpu::{NativeAdapterSelectorMethod, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use eframe::{egui, wgpu};
use std::sync::Arc;

/// `--renderer`: which drawing back-end to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RendererChoice {
    /// OpenGL, then DirectX when OpenGL cannot be set up.
    #[default]
    Auto,
    OpenGl,
    /// DirectX 12, the graphics card first.
    DirectX,
    /// DirectX 12 on its software adapter when there is one (what a machine without graphics driver gets).
    Software,
}

impl RendererChoice {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "opengl" => Self::OpenGl,
            "directx" => Self::DirectX,
            "software" => Self::Software,
            _ => return None,
        })
    }
}

/// One way of drawing the window to try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// `fail`: make the OpenGL set-up fail on purpose (`--simulate-no-opengl`, to test the fallback).
    OpenGl { fail: bool },
    DirectX { software_first: bool },
}

impl Attempt {
    pub fn name(self) -> &'static str {
        match self {
            Attempt::OpenGl { .. } => "OpenGL",
            Attempt::DirectX { .. } => "DirectX 12",
        }
    }
}

/// The attempts, in order, for a choice. `Auto` ends with DirectX so the window opens wherever it can.
pub fn attempts(choice: RendererChoice, simulate_no_opengl: bool) -> Vec<Attempt> {
    match choice {
        RendererChoice::Auto => vec![Attempt::OpenGl { fail: simulate_no_opengl }, Attempt::DirectX { software_first: false }],
        RendererChoice::OpenGl => vec![Attempt::OpenGl { fail: simulate_no_opengl }],
        RendererChoice::DirectX => vec![Attempt::DirectX { software_first: false }],
        RendererChoice::Software => vec![Attempt::DirectX { software_first: true }],
    }
}

/// Is this start-up error about the graphics (so another back-end may work) rather than about Strata itself?
pub fn graphics_failure(e: &eframe::Error) -> bool {
    matches!(e, eframe::Error::Glutin(_) | eframe::Error::NoGlutinConfigs(..) | eframe::Error::OpenGL(_) | eframe::Error::Wgpu(_))
}

/// Preference order of adapters: graphics cards first, then virtual ones, then the CPU (WARP); with
/// `software_first` the CPU adapter wins.
fn adapter_rank(kind: wgpu::DeviceType, software_first: bool) -> u8 {
    use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other, VirtualGpu};
    if software_first && kind == Cpu {
        return 0;
    }
    let by_kind = match kind {
        DiscreteGpu => 0,
        IntegratedGpu => 1,
        VirtualGpu => 2,
        Cpu => 3,
        Other => 4,
    };
    by_kind + u8::from(software_first)
}

fn adapter_selector(software_first: bool) -> NativeAdapterSelectorMethod {
    Arc::new(move |adapters, surface| {
        adapters
            .iter()
            .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
            .min_by_key(|a| adapter_rank(a.get_info().device_type, software_first))
            .cloned()
            .ok_or_else(|| "no graphics adapter can draw to this window".to_string())
    })
}

/// Window options for one attempt.
pub fn native_options(viewport: egui::ViewportBuilder, centered: bool, attempt: Attempt) -> eframe::NativeOptions {
    let base = eframe::NativeOptions { viewport, centered, ..Default::default() };
    match attempt {
        // An impossible depth buffer: no OpenGL configuration matches, as on a machine without OpenGL.
        Attempt::OpenGl { fail } => eframe::NativeOptions { renderer: eframe::Renderer::Glow, depth_buffer: if fail { 255 } else { 0 }, ..base },
        Attempt::DirectX { software_first } => eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: WgpuConfiguration {
                wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew { native_adapter_selector: Some(adapter_selector(software_first)), ..WgpuSetupCreateNew::without_display_handle() }),
                ..Default::default()
            },
            ..base
        },
    }
}

/// What DirectX picked, for the diagnostics.
pub fn adapter_text(info: &wgpu::AdapterInfo) -> String {
    format!("{:?} via wgpu | {} ({:?})", info.backend, info.name, info.device_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other, VirtualGpu};

    #[test]
    fn the_choice_names_are_parsed() {
        assert_eq!(RendererChoice::parse("AUTO"), Some(RendererChoice::Auto));
        assert_eq!(RendererChoice::parse("opengl"), Some(RendererChoice::OpenGl));
        assert_eq!(RendererChoice::parse("directx"), Some(RendererChoice::DirectX));
        assert_eq!(RendererChoice::parse("software"), Some(RendererChoice::Software));
        assert_eq!(RendererChoice::parse("vulkan"), None);
    }

    #[test]
    fn auto_tries_opengl_then_directx_and_the_others_try_one_thing() {
        assert_eq!(attempts(RendererChoice::Auto, false), vec![Attempt::OpenGl { fail: false }, Attempt::DirectX { software_first: false }]);
        assert_eq!(attempts(RendererChoice::Auto, true)[0], Attempt::OpenGl { fail: true });
        assert_eq!(attempts(RendererChoice::OpenGl, false), vec![Attempt::OpenGl { fail: false }]);
        assert_eq!(attempts(RendererChoice::DirectX, false), vec![Attempt::DirectX { software_first: false }]);
        assert_eq!(attempts(RendererChoice::Software, false), vec![Attempt::DirectX { software_first: true }]);
    }

    #[test]
    fn adapters_are_ranked_cards_first_and_the_cpu_last_unless_software_is_wanted() {
        let order = |software_first: bool| {
            let mut kinds = [Other, Cpu, VirtualGpu, IntegratedGpu, DiscreteGpu];
            kinds.sort_by_key(|k| adapter_rank(*k, software_first));
            kinds
        };
        assert_eq!(order(false), [DiscreteGpu, IntegratedGpu, VirtualGpu, Cpu, Other]);
        assert_eq!(order(true)[0], Cpu, "the software adapter first when asked");
        assert_eq!(order(true)[1], DiscreteGpu);
    }

    #[test]
    fn only_graphics_errors_trigger_the_fallback() {
        assert!(!graphics_failure(&eframe::Error::AppCreation("not graphics".into())));
    }
}
