# Chromaglyph

**把语法高亮编译进字体。** 给定一个静态 TTF、一个主题和内容已知（exact 模式）的代码块，
产出一个彩色 OpenType 字体：代码文本放进 DOM 后**字节不变**，高亮完全发生在字体内部的
glyph 替换里——不用 `<span>`，复制粘贴零污染。

灵感来自 [Cobble / FontWithASyntaxHighlighter](https://github.com/ssssota/FontWithASyntaxHighlighter)。
原版在字体里手写了一个「有限状态机词法分析器」（706 条 calt 规则逐字符推进状态）；
Chromaglyph 反其道而行：**内容在构建期已知，解析在构建期完成**，字体里只留下查表。

## 工作原理

```
tokenize → theme → compile → fontbuild → verify
```

1. **tokenize**：`(lang, text)` → token 列表（M1 是硬编码的演示 lexer，M2 换 syntect）。
2. **theme**：scope → CPAL 调色板索引（颜色去重；多主题 = 一个字体多调色板，M2）。
3. **compile**（核心创新）：每个被着色的 token 出现位置生成一条链式上下文替换规则
   （GSUB ChainContext format 2），上下文取**恰好能唯一锚定该出现位置的最小窗口**，
   并在全部块的全局范围内做假匹配审计：
   - 上下文类别（class）同时包含基础字形和全部彩色副本 → 规则与替换顺序无关；
   - 长 token（注释、字符串）按 64 字形的整形器上下文预算切段，链式接力；
   - 无法唯一区分的出现位置（如两个块内容完全相同但语言不同）降级为不染色并记入
     report.warnings——**构建永不失败**。
4. **fontbuild**：源字体表尽量按字节原样保留；彩色副本是**零轮廓字形** + COLRv0 层引用
   基础字形（每个颜色的体积成本 ≈ 0），替换掉源字体的 GSUB/GPOS/GDEF 和 AAT 布局表
   （避免干扰，M2 加 `--keep-base-features`）。v1 只收静态 TrueType（拒绝 CFF/OTF、变字体）。
5. **verify**（预言机）：rustybuzz 进程内整形产出的字体，逐字符比对替换后的 glyph id；
   再用 read-fonts 读回 COLR/CPAL 核对颜色。`build` 默认自跑，不合格直接报错。

## 状态：M1 walking skeleton ✅

- [x] cargo workspace：`chromaglyph-core`（纯函数库）+ `chromaglyph-cli`
- [x] 读静态 TTF → 彩色字体（COLRv0 + CPAL + calt GSUB）→ WOFF1
- [x] rustybuzz 预言机逐字符验证 + COLR/CPAL 颜色回读 + 浏览器像素级验证
- [x] 多块合并、全局消歧、64 字形上下文预算、降级不失败
- [x] 自我压力测试：喂 2000 行 Rust 源码（24815 字符）→ 744 条规则，验证全对
- [ ] M2：syntect tokenizer、TextMate/VS Code 主题导入、多调色板 + `@font-palette-values`、
      `--css`/`--report` 完整化、`--keep-base-features`
- [ ] M3：napi-rs 绑定 + remark/VitePress 插件（批处理合并、内容哈希缓存、失败静默降级）
- [ ] M4：tree-sitter 语义 token、cmap14/variation-selector 路线、任意代码的通用模式

## 用法

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf --lang rust \
    --input examples/hello.rs --output out.woff \
    [--css snippet.css] [--report report.json] [--no-verify]
chromaglyph verify --font out.woff --input examples/hello.rs --lang rust
```

```css
@font-face { font-family: 'chromaglyph'; src: url('out.woff') format('woff'); }
pre, code { font-family: chromaglyph, 'JetBrains Mono', monospace; }
```

输出扩展名选 `.ttf` 则写裸 sfnt，`.woff`（默认）写 WOFF1（zlib）。纯 Rust 生态还没有成熟的
WOFF2 编码器（需要 Brotli + woff2 变换栈），v1 先退 WOFF1，浏览器全部支持。

```sh
cargo build --release
cargo test                     # 17 tests: 规则生成单测 + 端到端（含 fixture 字体）
cargo run -p chromaglyph-cli -- build --font crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf \
    --input examples/hello.rs --output target/demo.woff --css target/demo.css
python3 -m http.server -d target/demo 8143   # 打开 index.html 看效果
```

## 尺寸对比（examples/hello.rs，114 字符）

| 表 | 本项目 | 参考实现 FontWithASyntaxHighlighter |
|----|-------|-------|
| GSUB | 1.3 KB（exact 模式，仅锚定规则） | 39 KB（FSM 全状态机） |
| COLR | 0.5 KB（空轮廓 + 层引用） | 12.7 KB（轮廓复制） |
| 规则数 | 10 | 706 |

自我压力测试（`compile.rs` 本身，24815 字符）：6217 个 token 去重后 744 条规则，
191 个彩色副本，ttf 290 KB / woff 117 KB，构建 1.8s。

## 技术选型

- 读字体 `read-fonts` / `skrifa`，写表 `write-fonts`（fontations 系）
- 整形预言机 `rustybuzz`（HarfBuzz 算法的纯 Rust 移植）
- WOFF1 `flate2`（zlib）

## License

Apache-2.0。测试 fixture `JetBrainsMono-Regular.ttf` 遵循其自身的 OFL 许可。
