# 主题

Chromaglyph 内置了 shiki 的
[tm-themes](https://github.com/shikijs/textmate-grammars-themes) 主题集：
和 VS Code、shiki 用的是同一批主题文件，逐字节原样 vendor 后压缩进二进制
（gzip 后约 250 KB）。本页上的任何名字都可以用在需要主题的地方：

- **插件**（`@chromaglyph/markdown-it`、`@chromaglyph/remark`）：
  `themes: ['github-dark', 'github-light']`。第一个主题是深色调色板，第二个是
  浅色——切换用的 CSS 变量见[接入 VitePress](./integration)。
- **CLI**：`--theme github-dark --theme github-light`；`chromaglyph themes`
  打印这份列表（脚本用 `--json`）。
- **Node**：`listThemes()` 返回注册表并附带各 scope 的色样，不编译字体也能做一个
  主题选择器。

三处都同样接受你自己的 VS Code `.json` 或 TextMate `.tmTheme` 文件路径。
`dark` / `light` 是 `dark-plus` / `light-plus` 的别名。

下面的预览是用各主题调色板着色的普通 HTML——关键字、字符串、注释、数字、函数、
类型、变量的颜色由真实的高亮器解析得出——而不是字体本身，方便在构建前比较主题。

<ThemeStats themes-label="个主题" version-label="主题集版本" />

## 深色

<ThemeGallery type="dark" source-label="上游" />

## 浅色

<ThemeGallery type="light" source-label="上游" />

## 致谢

每个内置主题都保留其作者的许可证——MIT 或 Apache-2.0；主题集里其它许可证的主题
（aurora-x，GPL-3.0）不随发行。上游的许可声明随主题文件一同 vendor 在
`crates/chromaglyph-core/themes/tm/NOTICE`。

<ThemeCredits theme-label="主题" license-label="许可证" source-label="来源" upstream-label="上游" />
