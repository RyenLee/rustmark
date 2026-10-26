# 变更说明

<!-- 一段话说明「改了什么、为什么」；评审先读这段。 -->

## 类型

- [ ] 缺陷修复（fix）
- [ ] 新功能（feat）
- [ ] 重构 / 结构改造（refactor）
- [ ] 文档（docs）
- [ ] 构建 / CI / 发布（chore）

## 涉及层与 crate

<!-- 勾选实际改动；跨层改动请在下面「风险」里说明依赖方向为何仍然单向。 -->

- [ ] `src/`（组合根 / 视图）
- [ ] `crates/rustmark-core`（domain / application）
- [ ] `crates/rustmark-ui`（presentation 非视图部分）
- [ ] `crates/rustmark-infra`（持久化 / 导出 / 解析 / 字体 / PDF）
- [ ] `crates/rustmark-shared` / `-actions` / `-icons`
- [ ] 测试 / 文档 / CI

## 验证（贴实测数字，不接受「已验证」）

```text
cargo fmt --check                                              →
cargo run --bin check-arch                                     →
cargo clippy --workspace --all-targets --all-features -- -D warnings →
cargo test --workspace --all-targets --no-fail-fast            →  passed / failed / ignored
cargo test --workspace --doc                                   →
python tools/check_thresholds.py                               →
bash scripts/size_gate.sh                                      →  B（红线 11 MiB）
```

- [ ] 涉及**守卫规则**的改动已做变异验证（把规则改坏 → 用例当场变红）
- [ ] 涉及**用户可见行为**的改动有真帧或领域级回归（不是只有纯函数单测）
- [ ] 新增 / 放宽 `pub` 字段已登记 `tests/field_visibility_guard_test.rs`
- [ ] 新增测试入口已在根 `Cargo.toml` 补 `[[test]]`（否则不会编译）
- [ ] 阈值有变动时已同步 `docs/thresholds.baseline.yaml`

## 风险与回滚

<!-- 最坏会怎么坏；坏了怎么退（revert 单 commit / 开关 / 保留旧实现并行）。 -->

## 关联

<!-- 关闭的 Issue、依据的 ADR / 报告条目（如 D-6、V11-61、SEC-03）。 -->
