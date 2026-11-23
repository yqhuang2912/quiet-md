# Document themes (CSS)

Each document theme is a CSS file. Choose **Edit CSS** under Settings › Appearance to open the selected theme in your editor. Saving the file redraws documents in all windows.

New installations use Feishu by default. Existing libraries retain their selection. The dropdown lists Feishu, Craft, Notion and Obsidian in that order. Selecting an option updates the page preview below it.

设置 › 外观 ›「图片与代码样式」可以单独选择「慢变量」，参考慢变量构建后阅读页的图片细边框、图注分隔线、暖灰代码块和语法配色。它适用于现有文档主题，覆盖图片、图注、行内代码及代码块；表格保留原主题的颜色、边框和单元格排版，在正文栏中居中，超宽时仍可横向滚动。正文排版和应用界面继续使用原主题。选择「跟随文档主题」即可恢复。已有图片的尺寸、裁剪和对齐设置保留；编辑模式中点击图片，可拖动四角手柄等比缩放（阅读模式不显示手柄）。代码块保留原生折叠、复制和换行工具栏。代码配色在 `assets/content-styles/manbianliang.css`，图片阅读样式在 `src/media_theme.rs`。

- Each built-in file in `assets/themes/` specifies the current value of every property and serves as a complete example.
- The first time you edit a built-in theme, the app copies it to `themes/<id>.css` in your library. The copy takes precedence and appears as modified in the menu. Restoring defaults deletes the copy.
- Duplicating a theme creates `themes/<id>-copy.css`. Rename it to make it your own. You can also drop someone else's CSS file into `themes/`. The app picks it up automatically.
- The parser skips invalid values and unknown selectors or properties. Settings lists the declarations that did not take effect.

## File structure

```css
:root {
  --theme-name: "Paper";              /* Required display name */
  --theme-name-en: "Paper";           /* Name in the English interface */
  --theme-author: "Your name";
  --theme-description: "A warm page"; /* Metadata, omitted from the menu */
  --theme-base: craft;                /* Block rendering style: craft or feishu */

  --accent: hsl(254, 80%, 68%);       /* Custom variable referenced with var() below */
  --color-red: #d44c47;               /* Named color, described below */
}

.page { color: #37352f; font-size: 16px; line-height: 1.5; max-width: 708px; }
h1 { font-size: 1.875em; font-weight: 600; margin-top: 32px; }
blockquote { border-left: 3px solid var(--accent); }
```

- The parser supports `/* comments */`, `var(--x, fallback)` and comma-separated selectors. It skips entire at-rules such as `@media`.
- Lengths accept `px` (points), `em` (relative to body text for font size and to the element's font size elsewhere) and `rem` (16). A unitless `line-height` is a multiplier.
- Colors accept `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, `transparent`, `white` and `black`. Use `inherit` on a color property to use the text color.
- `font-family` selects the first installed font in the list. `system-ui`, `-apple-system` and `sans-serif` select the system font. `ui-monospace` and `monospace` select the system monospace font.

## Selectors and properties

Themes with `--theme-base: feishu` use Feishu block rendering. The last column lists properties this renderer applies. A check mark means all listed properties apply. Settings flags other declarations as inactive.

| Selector | Properties | Controls | Feishu |
|---|---|---|---|
| `.page` (`body`) | `background-color` `color` `font-family` `font-size` `line-height` `caret-color` | Page and body text | ✓ |
| `.page` | `max-width` | Body column at standard page width. Wide and full widths are independent. | ✓ |
| `::selection` | `background-color` | Selected text background | ✓ |
| `p` | `margin-bottom` | Space between blocks | ✓ |
| `.title` | `font-family` `font-size` `line-height` `font-weight` `color` | Document title | ✓ |
| `h1`–`h9` | `font-family` `font-size` `line-height` `font-weight` `color` `margin-top` `margin-bottom` | Heading levels | Size, line height, weight and margins |
| `strong` (`b`) | `font-weight` `color` | Bold text | |
| `a` | `color` `text-decoration` (`underline` / `none`) `text-decoration-color` | Links. With `underline`, the line is always visible. | Color only |
| `a:hover` | `color` | Hovered link | ✓ |
| `code` | `color` `background-color` `border-color` `border-radius` `padding-left` | Inline code | |
| `pre` | `font-family` `font-size` `line-height` `color` `background-color` `border-color` `border-radius` | Code blocks | Colors and font |
| `.line-numbers` | `display` (`block` / `none`) `color` `background-color` | Code block line numbers | Colors |
| `.token.comment` `.token.keyword` `.token.tag` `.token.string` `.token.number` `.token.boolean` `.token.class-name` `.token.attr-name` `.token.function` | `color` | Syntax highlighting using Prism class names | ✓ |
| `blockquote` | `border-left` (or `border-left-width` / `-color`) `padding-left` `color` `font-style` | Quote rule, indentation and text | Rule color only |
| `ul` | `padding-left` | Child indentation | |
| `ul li::marker` / `ol li::marker` / `li::marker` | `color` | Bullet and list number colors | |
| `.task` (`input[type=checkbox]`) | `width` (also sets height) `border-radius` `border-color` | Task checkbox | Border color only |
| `.task:checked` | `background-color` `color` | Checked background and checkmark | |
| `.task-done` | `color` `text-decoration` (`line-through` / `none`) | Completed task text | Color only |
| `hr` | `border-top` (or `border-top-style` `-width` `-color`) | Divider (`dotted` / `solid` / `dashed`) | Color only |
| `table` (`td`) | `font-size` `line-height` `border-color` `border-radius` | Table text and rules | Border color only |
| `th` | `background-color` | Header background | ✓ |
| `.callout` | `border-radius` `border-width` `padding` (top / left) | Callout blocks | |

### Named colors in `:root`

Documents store text, background and callout colors by name. The theme supplies their displayed values.

| Variable | Purpose |
|---|---|
| `--color-gray` … `--color-purple` | Text (gray, red, orange, yellow, green, blue, purple) |
| `--highlight-light-red`, `--highlight-red`, … | Text highlights. Use `light-*` for the lighter shade and the unprefixed name for the darker shade. Gray also has `medium-gray`. |
| `--background-light-red`, `--background-medium-red`, … | Block backgrounds |
| `--callout-light-red`, `--callout-medium-red`, … | Callout fills |
| `--border-red`, … | Callout borders |

### Other document details (`--token-*`)

Override `--token-<name>` in `:root` for block hover and selection colors, table toolbars, the size picker and other details. The final section of each built-in theme lists the tokens and their current values.

## Scope

Themes style the document. The sidebar, menus and settings have their own appearance. Choose System, Light or Dark in Settings › Appearance to set the application appearance. CSS also leaves block structure, such as code toolbars and table row and column tools, under the editor's control.

Choosing an installed font in Settings › Appearance › Document font draws the body text, the title and the headings in it, so this file's `font-family` for `.page`, `.title` and `h1`–`h9` has no effect while one is chosen; Theme default goes back. `pre` keeps its own font.
