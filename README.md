# Chromaglyph

**把语法高亮编译进字体。** 给定一个静态 TTF、若干主题和内容已知（exact 模式）的代码块，
产出一个彩色 OpenType 字体：代码文本放进 DOM 后**字节不变**，高亮完全发生在字体内部的
glyph 替换里——不用 `<span>`，复制粘贴零污染。多主题 = 一个字体多调色板，明暗切换只是
一次 CSS `@font-palette-values` 翻转，零重排。

灵感来自 [Cobble / FontWithASyntaxHighlighter](https://github.com/ssssota/FontWithASyntaxHighlighter)。
原版在字体里手写了一个「有限状态机词法分析器」（706 条 calt 规则逐字符推进状态）；
Chromaglyph 反其道而行：**内容在构建期已知，解析在构建期完成**，字体里只留下查表。

## 工作原理

```
tokenize+theme → compile → fontbuild → verify
```

1. **tokenize + theme**：syntect（bat 的引擎，纯 Rust fancy-regex 构建，零 C 依赖）用真实
   TextMate 语法解析，再在**每个主题**下把每个字符解析成具体颜色（颜色 == 主题默认前景色
   视为「未着色」）。主题支持 VS Code JSON（含 JSONC、`include` 链合并）与 `.tmTheme`。
2. **compile**（核心创新，slot 模型）：一个 **slot** = 一个去重后的「逐主题颜色向量」；
   字符只要在任一主题下着色就属于联合结构。编译在**整形视图**上进行——变体选择符
   （emoji 后缀 FE0F 等）对整形器不可见，视为透明、不切断 run；无字形字符（换行、
   emoji）是硬屏障，规则上下文永不跨越（浏览器 fallback 会把它们移出本字体的整形
   run）；`base+VS` 可经源字体 cmap14（UVS）解析为变体字形。每个着色 run 生成一条
   链式上下文替换规则（GSUB ChainContext format 2），上下文取**恰好能唯一锚定该
   出现位置的最小窗口**，并在全部块的全局范围内做假匹配审计：
   - 上下文类别（class）同时包含基础字形和全部彩色副本 → 规则与替换顺序无关；
   - 长 token（注释、字符串）按 64 字形的整形器上下文预算切段，链式接力；
   - 含无字形字符的 token（行尾换行、emoji）在断点切开，周围继续染色；
   - 无法唯一区分的出现位置降级为不染色并记入 report.warnings——**构建永不失败**；
   - 因为规则只依赖 slot，**一套 GSUB 服务所有主题**，切换调色板零重排。
3. **fontbuild**：源字体表尽量按字节原样保留；彩色副本是**零轮廓字形** + COLRv0 层引用
   基础字形（每 slot 体积成本 ≈ 0），CPAL 一主题一调色板；替换掉源字体的 GSUB/GPOS/GDEF
   和 AAT 布局表（避免干扰）。v1 只收静态 TrueType（拒绝 CFF/OTF、变字体）。
4. **verify**（预言机）：rustybuzz 进程内整形产出的字体，逐字符比对替换后的 glyph id
   （整形与主题无关，验一次即可）；再按调色板逐一读回 CPAL 核对颜色。`build` 默认自跑。

## 状态

- [x] **M1** walking skeleton（见 git 历史）
- [x] **M2** CLI 完整化：
  - syntect tokenizer（Rust/JS/TS/Python/Go/… 全部内置语法，`--lang auto` 按扩展名探测）
  - 主题导入：VS Code `.json`（JSONC + `include` 链）与 `.tmTheme`；内置 `dark-plus` /
    `light-plus`（vendored 自 VS Code `theme-defaults`，MIT）
  - 多主题 = 多调色板 + 生成 `@font-palette-values` CSS（含 `prefers-color-scheme`
    明暗自动切换、背景色进 CSS 不进字体）
  - `--report` JSON、`verify` 子命令独立重验（从 COLR 反推结构，无需原字体）
  - 浏览器验证：同一 woff 双调色板明暗渲染正确
  - 自我压力测试：compile.rs 全文 24469 字符 → 482 条规则，双调色板 oracle 全绿
- [x] **M3** napi-rs 绑定 + remark 插件：
  - `@chromaglyph/node`（crates/chromaglyph-node）：`buildFont` / `buildFontSafe`
    （静默降级返回 null）/ 缓存 API；内容哈希 = sha256(字体+主题源+块文本+选项+版本)，
    进程内 LRU + 可选磁盘缓存（增量站点构建 49ms → 5ms）
  - `@chromaglyph/remark`（js/remark）：一个文档的所有 fenced 代码块**批处理合并进一个字体**
    （跨语言全局消歧），字体默认以 data: URI 内联（零配置），或 `embed: false` 落盘；
    CSS 作用域限定 `[data-chromaglyph]`（prose 永不受影响）；主题转
    `@font-palette-values`（双主题自动明暗切换）；任何失败静默降级
  - pnpm workspace（crates/chromaglyph-node + js/*）；6 个 JS 测试（unified 真实管线）
  - 浏览器验证：一个字体同时高亮 Rust/JS/Python 三个代码块，prose 内联代码不受影响
  - `@chromaglyph/markdown-it`（js/markdown-it，VitePress 通道）：wrap `md.render`
    （只 parse 一次、批处理全部 fence、从已解析 token 渲染）、链式接住已有 fence 规则、
    嵌套渲染隔离、`md.chromaglyph()` 自省；8 个测试
  - 共享 CSS 构建器移至 `@chromaglyph/node/css`（两个插件同源）
- **0.1.0 已发布**：crates.io `chromaglyph-core` / `chromaglyph`；npm
  `@chromaglyph/node`（+ darwin-arm64 平台包）、`@chromaglyph/remark`、
  `@chromaglyph/markdown-it`（其余平台二进制走 `.github/workflows/napi.yml` CI 矩阵）
- [x] **M4a** cmap14 / variation selector：
  - 整形视图：变体选择符（FE00–FE0F、180B–180F、E0100–E01EF）与零宽控制符
    （200B–200F）透明——不切断着色 run、不占规则窗口位置、不再误报 warning
  - 无字形字符是硬屏障：规则上下文（backtrack/lookahead）永不跨越（浏览器
    fallback 分段语义；此前跨屏障窗口「oracle 绿但浏览器不命中」的隐患修复）
  - cmap14（UVS）：源字体带 format 14 子表时原样透传，`base+VS` 解析为变体字形
    并按其建规则（e2e 用注入 cmap14 的 fixture 字体验证 oracle 全绿）
  - oracle 改为按 cluster 对齐（rustybuzz 会为 VS 多产 glyph、浏览器则删除，
    两者行为不同，cluster 对齐对两种实现都正确）
- [x] **M4b** 任意代码的通用模式（`--generic rust`，自动合成状态机，v1.1）：
  - **不再要求内容构建期已知**：一个字体高亮该语言的任意文本——关键字、行注释、
    双引号字符串，纯靠字形替换推进状态（思路即原项目的 706 条手写 calt FSM，
    这里从语言 spec 自动合成，且只要约 100 条规则）
  - 机制：传播规则（backtrack=状态类）逐字推进颜色；开引号进「字符串态」、
    闭引号替换进独立的「结束态」截断传播；`//` 开注释态；`.notdef` 独立成类，
    无字形字符=屏障（浏览器 fallback 分段，状态死亡）——与 exact 模式同一套
    整形语义
  - oracle：参考 lexer + 400 个确定性随机文本做性质测试，浏览器/整形器行为
    逐 glyph 对齐（含状态在屏障处死亡、关键字边界需要两侧存在已映射标点字形
    这类 shaper 真实能力）
  - v1.1：**数字状态**（`3.14` 用前瞻规则、`1.max` 的点不吞）；**块注释
    `/* */`**（开/闭规则）；当时引入的「注入映射 \n 的零宽字形保住跨行
    整形 run」机制后被 M4d 的浏览器实测推翻——跨行在真实浏览器中不可实现
  - 多主题依旧一套规则 + 每主题一个 CPAL 调色板
- [x] **M4d** 多语言 generic（v1.2）：`--generic` 支持 **js / ts / python /
  go / java**，spec 模型从「单引号字符」泛化为：
  - **多引号字符串**：JS 的 `"` `'` `` ` `` 各自成态、互不干扰（`'a"b'` 里
    双引号是字符串体）
  - **模板插值**（JS/TS）：`` `hi ${name} bye` `` 与编辑器一致——字符串段
    橙、`${`/`}` 保持字符串色、表达式内容无色、`}` 后恢复字符串；嵌套
    为平面语义（第一个 `}` 恢复）。`$`/`{`/`}` 因此获得独立类别（紧凑
    风格 `){return` 失去锚点——已知代价）。f-string 插值刻意不做：
    `f` 前缀在字体眼里是普通标识符，无法与 `r"..."` 原始串和
    `"{} {}".format()` 区分，做了会让后者回归
  - **转义**：`\` + 任意一字形以**双字形配对规则**整体替换为字符串体——
    整形器推进到被转义的引号时它已离开引号类，闭串规则不再命中；`\\`
    不吞闭串、`\"` 不提前闭串、串尾孤立 `\` 正常死亡。注释/三引号内
    `\` 无语义（按字面处理，`\"""` 边缘按文本闭包——已知边界）
  - **多定界 span**：行注释 / 块注释 / Python **三引号**统一为一种机制；
    `"""` 开启符按「长者优先」压过单引号字符串，`'''` 与 `"""` 互不闭合
    ——与真实 tokenizer 的最长匹配一致
  - **关键字分组**：一门语言多组 scope（`keyword.control` 紫色 vs
    `storage.type` 蓝色 vs `constant.language`），看起来像真语法文件
  - **`$` 等语言特有标识符字符**：`$return` 不是关键字
  - **逐语言的字面量语义**：Go 反引号原始字符串**无转义**（`escapes` 开关，
    反斜杠后照样闭串）、Go rune / Java char 字面量按单引号字符串着色、
    Java 类型关键字按 `storage.type.java` 渲染为 VS Code 的青绿色
  - **模型修正（浏览器像素实测）**：浏览器对 `pre` 逐行整形——每个源行是
    独立的 shaping run，`\n` 被强制换行消费，即使字体给它映射了字形，
    上下文规则也无法跨行；v1.1 的注入换行字形在真实浏览器从未生效
    （rustybuzz 把整段文本当一个 run 整形，会「验证」这个假象）。模型改为
    **`\n`/`\r` 恒为硬屏障**：所有状态在行尾死亡（块注释/模板字符串/
    docstring 只着色起始行），列 0 或行尾的关键字无色（无 in-run 锚点/
    前瞻字形），缩进一格即正常着色；注入机制与 `spans_newlines` 一并移除
  - oracle 对各语言跑「精选 + 按 spec 生成的 400 个确定性随机」样本，
    另加各语言对抗样本（引号交错、未闭合、关键字胶水、屏障 emoji）；
    oracle 的 `mapped` 按产出字体的 cmap 判定；node 侧
    `buildFont({generic: 'js'})` 同步暴露（语言名进内容哈希）
  - 浏览器验证（js/demo 的 generic.html，三字体一页，逐 token 像素统计
    正反例 25/25 对齐）：单行块注释/模板/三引号全程着色，跨行状态起始行
    着色、续行无色，缩进关键字与 `*/ let` 中缀关键字着色，列 0 关键字、
    `$use`、f-string 前缀无色，JS 控制流紫/声明蓝颜色分明
- [x] **M4c** tree-sitter 语义 token（`--semantic` / `buildFont({semantic: true})`，
 覆盖 Rust / JS / TS / Python）：
  - 可选 crate `chromaglyph-treesitter`（workspace 里唯一带 C 依赖的 crate，
    core 保持纯 Rust）；解析代码树 → 语义 span（函数定义/调用/方法、参数、
    类型、常量、枚举成员、字段）→ 覆盖在 syntect 语法色之上，主题不认识
    的 scope 自动跳过——看起来像开了语言服务器的编辑器
  - core 只暴露 `BuildInput.semantic` 回调钩子（`SemanticLayer` 类型），
    与具体解析器完全解耦；oracle 全链路验证（新颜色→新 slot→新规则）
- **M4 收官**。范围决策记录：
  - 多语言同字体**刻意不做**：跨语言假阳性无法接受——合并 spec 后
    TS 的 `type` 会给 Rust 块里的 `type` 上色、Python 的 `is` 会命中
    JS/Java 文本；exact 模式（内容已知）已经是多语言一字体，generic
    保持一语言一字体

## 安装

- **预编译二进制**：GitHub Releases 提供 7 个目标（macOS arm64/x64、
  Linux x64/arm64 的 gnu/musl、Windows x64），每个版本附带 `SHA256SUMS`；
  musl 构建为全静态，适合容器。macOS 包为 ad-hoc 签名，浏览器下载后
  首次运行可能需要 `xattr -d com.apple.quarantine <binary>`。
- **cargo**：`cargo install chromaglyph`（crates.io）。
- **npm**：`@chromaglyph/node`（含 remark / markdown-it 插件）。

## 用法

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input examples/hello.rs \            # --lang auto 按扩展名探测
    --theme dark-plus --theme light-plus \ # 或 my-theme.json / my.tmTheme，可重复
    --output out.woff --css snippet.css --report report.json
chromaglyph verify --font out.woff --input examples/hello.rs

# 通用模式：不指定内容，字体高亮该语言的任意文本（关键字/注释/字符串/数字；
# rust | js | ts | python | go | java；着色为行内语义——见 M4d 的浏览器实测说明）
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

生成的 CSS（节选）：

```css
@font-face { font-family: 'chromaglyph'; src: url('out.woff') format('woff'); }
@font-palette-values --chromaglyph-dark  { font-family: chromaglyph; base-palette: 0; }
@font-palette-values --chromaglyph-light { font-family: chromaglyph; base-palette: 1; }
pre, code { font-family: chromaglyph, monospace; background: #1e1e1e; }
@media (prefers-color-scheme: light) {
  pre, code { font-palette: --chromaglyph-light; background: #ffffff; }
}
```

输出扩展名选 `.ttf` 则写裸 sfnt，`.woff`（默认）写 WOFF1（zlib）。纯 Rust 生态还没有成熟的
WOFF2 编码器（需要 Brotli + woff2 变换栈），v1 先退 WOFF1，浏览器全部支持。

```sh
cargo build --release
cargo test                                        # 61 tests（含 6 语言 generic oracle + 4 语言语义层）
cargo run -p chromaglyph -- build --font crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf \
    --input examples/hello.rs --output target/demo/chromaglyph-demo.woff --css target/demo/chromaglyph-demo.css
python3 -m http.server -d target/demo 8143        # 打开 index.html 看明暗双调色板
```

## 尺寸对比（examples/hello.rs，114 字符，双主题）

| 表 | 本项目 | 参考实现 FontWithASyntaxHighlighter |
|----|-------|-------|
| GSUB | 1.2 KB（exact 模式，仅锚定规则） | 39 KB（FSM 全状态机） |
| COLR | 0.4 KB（空轮廓 + 层引用） | 12.7 KB（轮廓复制） |
| CPAL | 40 B（2 调色板 × 6 slot） | 50 B（1 调色板） |
| 规则数 | 9 | 706 |

自我压力测试（`compile.rs` 本身，31535 字符，双主题）：622 条规则、7 slot、241 彩色副本，
构建 0.14s。全局假匹配审计基于字形序列的后缀数组索引（O(n log n) 查询，候选按
occurrence 缓存），126k 字符输入下构建 0.54s，输出与逐位置扫描版字节级一致。

## 技术选型

- 读字体 `read-fonts` / `skrifa`，写表 `write-fonts`（fontations 系）
- 整形预言机 `rustybuzz`（HarfBuzz 算法的纯 Rust 移植）
- 语法/主题 `syntect`（`default-fancy`：fancy-regex，零 C 依赖）
- WOFF1 `flate2`（zlib）

## License

Apache-2.0。`themes/*.json` 与测试 fixture `JetBrainsMono-Regular.ttf` 遵循其自身的
上游许可（VS Code theme-defaults MIT；JetBrains Mono OFL）。
