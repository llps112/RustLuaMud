# 代码质量与提交规范

## 修改/新增代码的完整检查流程

每次修改或新增 Rust 代码后，**在提交推送之前**必须依次执行以下三项检查：

### 1. 格式化检查

```bash
cargo fmt --all -- --check
```

> 如果格式化不通过，先执行 `cargo fmt --all` 自动修正，再重新检查。

### 2. Clippy Lint 检查

```bash
cargo clippy -- -D warnings
```

> 必须**零 warning、零 error** 才允许提交。`-D warnings` 将 warning 视为 error，和 CI 行为一致。

### 3. 测试

```bash
cargo test
```

> 必须全部通过。测试失败时先排查失败原因，修复后再提交。

---

## 完整提交流程

1. 修改代码
2. 依次运行：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`
3. 全部通过后，`git add` + `git commit` + `git push`
4. Push 后确认 GitHub Actions CI 也绿色通过
