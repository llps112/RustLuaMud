#!/usr/bin/env python3
"""
命名空间重构遗漏检测工具

检测"真正的冲突"：同一字段有时用 ns.field 赋值，有时用裸 field 赋值/读取。
这种不一致会导致赋值落在裸全局上，命名空间字段仍保留旧值，引发难查的 bug。

用法:
    python3 tools/check_ns_leak.py [lua_dir]

    lua_dir 默认 scripts/class-utf8

退出码:
    0 - 无冲突
    1 - 发现冲突
"""
import re
import os
import sys
from collections import defaultdict

# 预定义命名空间（michen_var.lua 里定义为 table 的）
# 脚本会自动从 michen_var.lua 补充，这里只放最基本的
BASE_NS = {
    'kill', 'fj', 'mp', 'ftb', 'war', 'yb', 'me', 'hp', 'stat', 'count',
    'sum', 'avg', 'common', 'always', 'jifa', 'skills', 'have', 'gxd',
    'dm', 'cm', 'xx', 'yp', 'workflow', 'mpLimited', 'addneili', 'mark',
    'setting', 'gps', 'xkxGPS', 'dl', 'sys', 'checkmove', 'checkbusy',
    'battle_idle_manager', 'burnnpc', 'mmr', 'bdz', 'fs', 'qzwd',
    'always_daytime', 'always_hp', 'always_skills', 'always_chkmj',
    'always_watch', 'always_items', 'watch_death',
}

LUA_KEYWORDS = {
    'function', 'end', 'if', 'for', 'while', 'return', 'do', 'then',
    'else', 'elseif', 'repeat', 'until', 'break', 'goto', 'in', 'and',
    'or', 'not', 'nil', 'true', 'false', 'self', 'local',
}


def read_lines(path):
    """尝试多种编码读取文件"""
    for enc in ('utf-8', 'gbk', 'latin-1'):
        try:
            with open(path, encoding=enc) as f:
                return f.readlines()
        except (UnicodeDecodeError, FileNotFoundError):
            continue
    return []


def auto_discover_namespaces(lua_dir):
    """从 michen_var.lua 自动解析命名空间定义（xxx={} 或 xxx={）"""
    ns_set = set(BASE_NS)
    var_file = os.path.join(lua_dir, 'michen_var.lua')
    if not os.path.exists(var_file):
        return ns_set
    lines = read_lines(var_file)
    for line in lines:
        # 匹配 name={} 或 name={ 或 name = {
        m = re.match(r'^(\w+)\s*=\s*\{', line)
        if m:
            name = m.group(1)
            if name not in LUA_KEYWORDS:
                ns_set.add(name)
    return ns_set


def strip_lua_comment(line):
    """去掉行内注释（粗略：找 -- 但不在字符串内）"""
    # 简化处理：找不在引号内的 --
    in_squote = False
    in_dquote = False
    i = 0
    while i < len(line):
        c = line[i]
        if c == '\\' and i + 1 < len(line):
            i += 2
            continue
        if c == "'" and not in_dquote:
            in_squote = not in_squote
        elif c == '"' and not in_squote:
            in_dquote = not in_dquote
        elif c == '-' and i + 1 < len(line) and line[i + 1] == '-' and not in_squote and not in_dquote:
            return line[:i]
        i += 1
    return line


def is_table_field_line(stripped, prev_lines):
    """判断是否是表构造器字段（如 key = value,）"""
    # 以逗号结尾 → 表字段
    if stripped.endswith(','):
        return True
    # 前一行以 { 结尾 → 表的第一行字段
    if prev_lines and prev_lines[-1].rstrip().endswith('{'):
        return True
    return False


def main():
    lua_dir = sys.argv[1] if len(sys.argv) > 1 else 'scripts/class-utf8'
    if not os.path.isdir(lua_dir):
        print(f"错误: 目录不存在 {lua_dir}")
        return 1

    known_ns = auto_discover_namespaces(lua_dir)

    # field -> {ns1, ns2, ...}：记录哪些命名空间有 ns.field = 赋值
    field_to_ns = defaultdict(set)
    # (relpath, lineno, field, content, prev_line) 裸赋值候选
    bare_assigns = []

    # 第一遍：收集所有 ns.field = 赋值
    for dirpath, dirs, files in os.walk(lua_dir):
        for fname in sorted(files):
            if not fname.endswith('.lua'):
                continue
            filepath = os.path.join(dirpath, fname)
            relpath = os.path.relpath(filepath, lua_dir)
            lines = read_lines(filepath)
            for i, line in enumerate(lines, 1):
                code = strip_lua_comment(line)
                for m in re.finditer(r'\b(\w+)\.(\w+)\s*=\s', code):
                    ns, field = m.group(1), m.group(2)
                    if ns in known_ns and field not in LUA_KEYWORDS:
                        field_to_ns[field].add(ns)

    # 第二遍：检测裸 field = 赋值
    for dirpath, dirs, files in os.walk(lua_dir):
        for fname in sorted(files):
            if not fname.endswith('.lua'):
                continue
            filepath = os.path.join(dirpath, fname)
            relpath = os.path.relpath(filepath, lua_dir)
            lines = read_lines(filepath)
            prev_lines_buf = []
            for i, line in enumerate(lines, 1):
                stripped = line.strip()
                code = strip_lua_comment(line)
                code_stripped = code.strip()

                # 收集前几行用于上下文判断
                prev_lines_buf.append(line.rstrip())
                if len(prev_lines_buf) > 3:
                    prev_lines_buf.pop(0)

                if not code_stripped or code_stripped.startswith('--'):
                    continue

                # 匹配裸赋值: 行首缩进 + word = ...
                m = re.match(r'^\s+(\w+)\s*=\s', code)
                if not m:
                    continue
                field = m.group(1)
                if field in LUA_KEYWORDS or len(field) < 2:
                    continue
                # 排除 local 声明
                if re.match(r'^\s*local\s', code):
                    continue
                # 排除 ns.field = 形式
                if re.match(r'^\s+\w+\.\w+\s*=', code):
                    continue
                # 排除表构造器字段
                if is_table_field_line(code_stripped, prev_lines_buf[:-1]):
                    continue
                # 排除 function 定义（field = function）
                if re.match(r'^\s+\w+\s*=\s*function\b', code):
                    continue

                bare_assigns.append((relpath, i, field, code_stripped))

    # 找冲突：裸赋值的 field 同时也有 ns.field 赋值
    conflicts = []
    for relpath, lineno, field, content in bare_assigns:
        if field in field_to_ns:
            namespaces = field_to_ns[field]
            conflicts.append((relpath, lineno, field, namespaces, content))

    print("=" * 70)
    print("命名空间重构遗漏检测")
    print(f"扫描目录: {lua_dir}")
    print(f"已知命名空间: {len(known_ns)} 个")
    print("=" * 70)

    if not conflicts:
        print("\n✅ 未发现命名空间冲突（所有字段读写一致）。\n")
        return 0

    # 按字段分组
    by_field = defaultdict(list)
    for c in conflicts:
        by_field[c[2]].append(c)

    print(f"\n❌ 发现 {len(conflicts)} 处冲突（{len(by_field)} 个字段名）：")
    print("    这些字段有时用 ns.field 赋值，有时用裸 field 赋值，")
    print("    裸赋值不会更新命名空间字段，会导致状态不一致。\n")

    for field in sorted(by_field.keys()):
        entries = by_field[field]
        namespaces = sorted(set().union(*[set(e[3]) for e in entries]))
        ns_list = ' / '.join(f'{ns}.{field}' for ns in namespaces)
        print(f"── {field}  (应为 {ns_list}) ──")
        for relpath, lineno, _, _, content in entries:
            print(f"  {relpath}:{lineno}: {content}")
        print()

    return 1


if __name__ == '__main__':
    sys.exit(main())
