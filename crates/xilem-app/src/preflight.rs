//! 启动期 GPU 预检：无硬件 GPU 时给出原生报错而非 vello-on-WARP 访问违例崩溃
//! （spec 2026-10-15 §6，依据附录 A.1 崩溃实测）。

/// 按适配器类型给出拒绝文案；硬件适配器返回 None（通过）。
fn adapter_rejection(device_type: wgpu::DeviceType) -> Option<&'static str> {
    match device_type {
        wgpu::DeviceType::Cpu => Some("此应用需要硬件 GPU。无 GPU 环境请使用网页版（自动软渲染）"),
        // VirtualGpu（远程桌面虚拟适配器）有真实 GPU 后端，放行；
        // Other（无法分类）放行——交给 masonry 现有路径处理。
        _ => None,
    }
}

/// 枚举适配器并判定。Err 时宿主弹原生消息框后退出。
pub(crate) fn preflight_gpu() -> Result<(), String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|_| "未找到可用的图形适配器。无 GPU 环境请使用网页版".to_string())?;
    if let Some(msg) = adapter_rejection(adapter.get_info().device_type) {
        return Err(msg.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::adapter_rejection;
    use wgpu::DeviceType;

    #[test]
    fn hardware_adapters_pass() {
        assert!(adapter_rejection(DeviceType::DiscreteGpu).is_none());
        assert!(adapter_rejection(DeviceType::IntegratedGpu).is_none());
    }

    #[test]
    fn cpu_adapter_rejected_with_message() {
        let msg = adapter_rejection(DeviceType::Cpu).expect("WARP/lavapipe must be rejected");
        assert!(msg.contains("GPU"), "message should mention GPU: {msg}");
    }
}
