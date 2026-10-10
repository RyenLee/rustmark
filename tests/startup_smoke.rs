//! 启动冒烟：守住「能编译、能通过全部测试、但启动即 panic」这类**配置型**缺陷。
//!
//! # 为什么必须有这一条
//!
//! DEF-03 的表现是：release 二进制一启动就 panic
//! （`No wgpu backend feature that is implemented for the target platform was enabled.`）。
//! 根因是 eframe 关掉 `default-features` 后，`eframe/default → egui-wgpu/default →
//! wgpu/default` 这条链断开，导致 wgpu 的**后端 feature**（dx12 / metal / vulkan / gles）
//! 一个都没启用。
//!
//! 这个缺陷能躲过 1114 条测试，是因为：
//!
//! - 单元测试不创建窗口；
//! - 表现层测试用 `egui::Context::default()` 直接画，走的是 CPU 布局，不碰 wgpu；
//! - 连 headless 驱动 `EditorApp::update`（S7-06 收尾引入）也不需要真实 `Frame`。
//!
//! 也就是说，**整个测试体系里没有任何一条路径会真正创建 wgpu 实例**。
//!
//! # 本文件的守卫点
//!
//! `wgpu::Instance::new` panic 的**唯一**前置条件是「当前目标平台没有启用任何后端
//! feature」，而 `enabled_backend_features()` 是纯 `cfg` 判定——
//! 不需要显示器、不需要窗口、不需要 GPU。因此可以在任何 CI runner 上稳定断言。
//!
//! 它不能直接证明「应用能起来」，但能挡住 DEF-03 这一类**必然**导致起不来的配置错误；
//! 真正的「启动一次」冒烟见 `scripts/smoke_startup.ps1`（需 GUI 会话，Windows 手工/CI）。

#[test]
fn wgpu_has_at_least_one_backend_enabled() {
    let backends = wgpu::Instance::enabled_backend_features();
    assert!(
        !backends.is_empty(),
        "wgpu 未启用任何后端 feature：这会让 `wgpu::Instance::new` 在启动时 panic（DEF-03）。\n\
         典型成因：对 eframe 关掉了 default-features，导致\n\
         `eframe/default → egui-wgpu/default → wgpu/default`\n\
         （dx12 / metal / vulkan / gles 的唯一来源）未被启用，而 Cargo.toml 又没有按平台补上。\n\
         修复方式：见 Cargo.toml 中 `wgpu` 的按平台声明。\n\
         注意：本断言通过 ≠ 应用一定能起来，它只排除这一类配置缺陷。"
    );
    eprintln!("[smoke] 当前目标平台已启用的 wgpu 后端：{backends:?}");
}
