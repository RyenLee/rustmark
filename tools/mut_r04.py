#!/usr/bin/env python3
"""R-04 用户可见层变异验证：把 Revert / InsertImage 的接线与编排改坏，
确认新增用例当场变红。

覆盖两侧行为（不只是路由）：
- Revert：路由空转、脏文档不确认、不清草稿、不清脏标记、不读盘内容、
  确认文案丢文档名；
- InsertImage：路由空转、导入失败静默、链接格式错、重名去重失效、
  未命名文档的相对/绝对分支丢失。

每个变异从干净基准重新生成、跑测试、try/finally 还原。
失败形态分类（与 mut_arch_r02.py 同口径，另加 captured_build）：
- captured：期望的用例变红；
- captured_build：变异触发 build.rs 架构校验先 panic，用例没机会跑；
- NO_TEETH：用例仍全绿（守卫失效，最严重）；
- COMPILE_FAIL / ANCHOR_MISS / NO_RESULT：变异本身无效。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = "app"

ACTIONS = os.path.join(ROOT, "src", "app", "frame", "actions.rs")
FILES = os.path.join(ROOT, "src", "app", "frame", "files.rs")
DIALOG = os.path.join(ROOT, "src", "app", "frame", "dialog.rs")
IMAGES = os.path.join(ROOT, "src", "app", "frame", "images.rs")
LIFECYCLE = os.path.join(ROOT, "src", "application", "document", "service", "lifecycle.rs")
IMAGE_STORE = os.path.join(ROOT, "src", "infrastructure", "persistence", "image_store.rs")

# (文件, 名称, 旧串, 新串, 期望被捕获的测试名)
MUTANTS = [
    # —— Revert：路由与确认流 ——
    (
        ACTIONS,
        "route_revert 空转（菜单/面板/快捷键全失效）",
        "fn route_revert(app: &mut EditorApp) {\n    app.revert_document();\n}",
        "fn route_revert(_app: &mut EditorApp) {}",
        "revert_restores_the_disk_content",
    ),
    (
        FILES,
        "脏文档不确认直接还原（丢掉不可逆操作的保护）",
        "        if self.current_document_dirty() {\n            self.request_revert(id, &path);\n            return;\n        }\n        self.run_revert(id);",
        "        self.run_revert(id);",
        "revert_of_dirty_document_asks_first",
    ),
    (
        FILES,
        "还原后不清草稿（下次启动把丢弃的修改又捡回来）",
        "                let _ = self\n                    .services\n                    .documents\n                    .clear_draft(&self.services.drafts_dir, id);\n                if self.active_doc == Some(id) {",
        "                if self.active_doc == Some(id) {",
        "revert_clears_the_stale_draft",
    ),
    (
        DIALOG,
        "还原确认文案丢文档名（用户不知道丢的是哪份）",
        '            "「{}」有未保存的修改，「还原」将丢弃这些修改并从磁盘重新加载。确定继续吗？",\n            Self::display_name(path.as_str())',
        '            "确定继续吗？"',
        "the_revert_message_names_the_document",
    ),
    # —— Revert：应用层用例 ——
    (
        LIFECYCLE,
        "还原后不 mark_saved（内容回到磁盘却仍显示脏点）",
        "            // `NotDirty`（替换无效果且文档本就干净）视为成功：内容与磁盘\n            // 一致正是本用例的目标状态，不是失败。\n            match doc.mark_saved(path, now) {\n                Ok(()) => {}\n                Err(DocumentError::NotDirty) => {}\n                Err(e) => return Err(e.into()),\n            }",
        "            let _ = (path, now);",
        "revert_restores_the_disk_content",
    ),
    (
        LIFECYCLE,
        "replace 传空串（不读盘，把文档清空）",
        "            doc.replace(range, &snapshot.content, now)?;",
        '            doc.replace(range, "", now)?;',
        "revert_restores_the_disk_content",
    ),
    # —— InsertImage ——
    (
        ACTIONS,
        "route_insert_image 空转（工具栏「插入图片」失效）",
        "fn route_insert_image(app: &mut EditorApp) {\n    app.insert_image();\n}",
        "fn route_insert_image(_app: &mut EditorApp) {}",
        "insert_image_copies_into_assets_and_links_relatively",
    ),
    (
        IMAGES,
        "导入失败静默（用户以为插成功了）",
        '                Err(e) => {\n                    self.set_status_error(&format!("导入图片失败：{e}"));\n                    return;\n                }',
        "                Err(_e) => return,",
        "import_failure_is_reported_and_nothing_is_inserted",
    ),
    (
        IMAGES,
        "链接格式错（插裸路径而非 ![]() 图片语法）",
        '        let markdown = format!("![]({link})");',
        '        let markdown = format!("{link}");',
        "insert_image_copies_into_assets_and_links_relatively",
    ),
    (
        IMAGES,
        "未命名文档丢失绝对路径分支（插空链接）",
        "            None => path.as_str().to_string(),",
        "            None => String::new(),",
        "insert_into_unsaved_document_uses_absolute_path",
    ),
    (
        IMAGE_STORE,
        "重名去重失效（同名永远落在原路径上，覆盖旧图）",
        "    if !candidate.exists() {\n        return Ok(candidate);\n    }",
        "    if true {\n        return Ok(candidate);\n    }",
        "second_insert_of_same_name_dedupes",
    ),
]


def read(p):
    with io.open(p, encoding="utf-8", newline="") as f:
        return f.read()


def write(p, s):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(s)


def run_tests():
    r = subprocess.run(
        ["cargo", "test", "--test", TARGET],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = r.stdout + r.stderr
    if "架构边界校验失败" in out:
        return "BUILD_GUARD", out
    if "test result:" not in out:
        # 没有 test result 行 = 测试目标没跑起来。若输出里有 `error`
        # （含 `error:` 这种非 `error[` 的编译错，例如 `format!` 多给了一个
        # 用不到的参数），就是编译失败；否则才是真·无结果。
        return ("COMPILE_FAIL" if "error" in out else "NO_RESULT"), out
    line = [l for l in out.splitlines() if l.startswith("test result:")][-1]
    ok = "0 failed" in line
    return ("PASS" if ok else "FAIL"), out


def main():
    bases = {}
    for path in dict.fromkeys(m[0] for m in MUTANTS):
        bases[path] = read(path).replace("\r\n", "\n")
    rows = []
    try:
        for path, name, old, new, expect in MUTANTS:
            # 每个变异都从**所有**文件的干净基准重新落盘，防止上一轮
            # 的改动残留（虽然 try/finally 保证还原，双保险）。
            for p, base in bases.items():
                write(p, base)
            cur = read(path)
            if old not in cur:
                rows.append((name, "ANCHOR_MISS", "锚点未命中：变异未生效"))
                continue
            write(path, cur.replace(old, new, 1))
            status, out = run_tests()
            if status == "COMPILE_FAIL":
                rows.append((name, "COMPILE_FAIL", "编译失败——变异无效"))
            elif status == "BUILD_GUARD":
                rows.append((name, "captured_build", "build.rs 架构校验拦下"))
            elif status == "NO_RESULT":
                rows.append((name, "NO_RESULT", "无 test result 行"))
            else:
                # 集成测试（`tests/app/main.rs` 的 `mod xxx_test`）输出形如
                # `test revert_test::revert_restores_the_disk_content ... FAILED`，
                # 真实测试名夹在 `::` 之后，故用「测试名 + 分隔符」做子串匹配，
                # 而不是「test 测试名」——后者因模块前缀永远命中不了。
                caught = f"{expect} ... FAILED" in out
                if caught:
                    rows.append((name, "captured", status))
                else:
                    failed = [l.strip() for l in out.splitlines() if " ... FAILED" in l]
                    note = "NO_TEETH；实际失败：" + "; ".join(failed) if failed else "NO_TEETH（无任何测试失败）"
                    rows.append((name, "NO_TEETH", note))
    finally:
        for p, base in bases.items():
            write(p, base)

    print(f"{'变异':<44} {'结果':<14} 测试状态")
    print("-" * 84)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<44} {res:<14} {note}")
        if res in ("captured", "captured_build"):
            teeth += 1
    print("-" * 84)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
