#!/usr/bin/env bash
#
# bump.sh — chromaglyph 版本升级脚本
#
# 本仓库有两条独立版本线(见 CHANGELOG 0.1.2:js 插件可与原生线不同步):
#
#   原生线(默认)——一个 v* tag 触发整条发布流水线,以下全部同步到同一版本:
#     - Cargo workspace 版本(root Cargo.toml,4 个 crate 继承它)
#     - chromaglyph-core / chromaglyph-treesitter 两处 path 依赖的 version 要求
#     - @chromaglyph/node 版本 + 7 个平台包 optionalDependencies pin
#     - npm/@chromaglyph/node-* 平台包模板(CI 发布时会再写一次版本)
#     - js/cli(chromaglyph npm 版 CLI 启动器包)+ 7 个 @chromaglyph/cli-*
#       平台包 optionalDependencies pin 与 npm/@chromaglyph/cli-* 模板
#     - Cargo.lock / pnpm-lock.yaml 再生成(CI 用 --frozen-lockfile,必须同步)
#     - CHANGELOG.md 的 "## Unreleased" 折叠为 "## X.Y.Z (日期)"
#
#   JS 线(--js)——五个 JS 包要求当前版本一致,一起升:
#     @chromaglyph/remark + @chromaglyph/markdown-it(核心插件)
#     @chromaglyph/astro / docusaurus / eleventy(框架适配器)
#
# 用法:
#   scripts/bump.sh [--dry-run] [--tag] [-m 描述] <major|minor|patch|X.Y.Z>
#   scripts/bump.sh --js [--dry-run] <major|minor|patch|X.Y.Z>
#
# 选项:
#   --dry-run   只打印计划,不修改任何文件
#   --tag       生成 release 提交并打 v* tag(仅原生线;不会 push——
#               push tag 会触发 napi.yml + cli-release.yml 公开发布)
#   -m <描述>   release 提交信息:"release X.Y.Z: <描述>"
#   --force     工作区不干净时仍继续(默认拒绝)
#
# 示例:
#   scripts/bump.sh patch                      # 0.1.2 -> 0.1.3,只改文件
#   scripts/bump.sh --tag -m "generic mode fixes" 0.2.0
#   scripts/bump.sh --js patch                 # js 插件各自 patch 一步

set -euo pipefail

cd "$(dirname "$0")/.."

SEMVER_RE='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'

usage() {
  sed -n '3,33p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

die() { echo "错误: $*" >&2; exit 1; }

# ---------------------------------------------------------------- 参数解析

DRY=0 TAG=0 FORCE=0 JS=0 DESC="" BUMP=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --tag)     TAG=1 ;;
    --force)   FORCE=1 ;;
    --js)      JS=1 ;;
    -m)        [ $# -ge 2 ] || die "-m 需要一个描述参数"; DESC="$2"; shift ;;
    -h|--help) usage ;;
    -*)        die "未知选项: $1(试试 --help)" ;;
    *)         [ -z "$BUMP" ] || die "只能给一个版本参数"; BUMP="$1" ;;
  esac
  shift
done
[ -n "$BUMP" ] || usage 1 >&2

# ---------------------------------------------------------------- 前置检查

if [ "$DRY" -eq 0 ] && [ "$FORCE" -eq 0 ] && [ -n "$(git status --porcelain)" ]; then
  die "工作区不干净(bump 结果要进 release 提交)。提交或暂存后重试,或 --force"
fi

if [ "$JS" -eq 1 ] && [ "$TAG" -eq 1 ]; then
  die "--tag 仅支持原生线:CI 只在 v* tag 上发布,js 线单独发版请手动 publish"
fi

case "$BUMP" in
  major|minor|patch) ;;
  *) echo "$BUMP" | grep -Eq "$SEMVER_RE" || die "版本格式应为 X.Y.Z(可带预发布后缀)或 major|minor|patch" ;;
esac

# ---------------------------------------------------------------- 版本计算

# root Cargo.toml 里唯一行首 version = 即 [workspace.package] 版本
native_base() {
  perl -ne 'print "$1\n" if /^version = "([^"]+)"$/' Cargo.toml
}

resolve_new() { # $1=基准版本 $2=major|minor|patch|X.Y.Z
  local plain="${1%%-*}" major minor patch
  major="${plain%%.*}"; plain="${plain#*.}"; minor="${plain%%.*}"; patch="${plain#*.}"
  case "$2" in
    major) echo "$((10#$major + 1)).0.0" ;;
    minor) echo "$major.$((10#$minor + 1)).0" ;;
    patch) echo "$major.$minor.$((10#$patch + 1))" ;;
    *)     echo "$2" ;;
  esac
}

JS_PKGS="remark markdown-it astro docusaurus eleventy"

if [ "$JS" -eq 1 ]; then
  BASE=$(node -p 'require("./js/remark/package.json").version')
  for p in $JS_PKGS; do
    V=$(node -p "require(\"./js/$p/package.json\").version")
    [ "$V" = "$BASE" ] || die "@chromaglyph/$p($V) 与 @chromaglyph/remark($BASE) 版本已分叉,请先手动对齐"
  done
else
  BASE=$(native_base)
  [ "$(printf '%s\n' "$BASE" | grep -c .)" -eq 1 ] \
    || die "root Cargo.toml 应恰好有一行行首 version =(当前不止一处),请人工确认"
fi

NEW=$(resolve_new "$BASE" "$BUMP")
[ "$NEW" != "$BASE" ] || die "新版本与当前版本相同: $NEW"
export NEW

if [ "$TAG" -eq 1 ] && git rev-parse -q --verify "refs/tags/v$NEW" >/dev/null; then
  die "tag v$NEW 已存在(上次发过或半途中断),请先处理: git tag -d v$NEW"
fi

# ---------------------------------------------------------------- 修改动作

edit() { # edit <说明> <命令...>:dry-run 时只打印
  local desc="$1"; shift
  if [ "$DRY" -eq 1 ]; then echo "  [dry-run] $desc"; else "$@"; fi
}

pkg_set_version() { # $1=package.json 路径  $2=1 时同步 pin optionalDependencies
  PKG_FILE="$1" PKG_VER="$NEW" PKG_PIN="${2:-0}" edit "版本 -> $NEW: $1" node <<'EOF'
const fs = require("fs");
const { PKG_FILE, PKG_VER, PKG_PIN } = process.env;
const j = JSON.parse(fs.readFileSync(PKG_FILE, "utf8"));
j.version = PKG_VER;
if (PKG_PIN === "1" && j.optionalDependencies) {
  for (const k of Object.keys(j.optionalDependencies)) {
    if (/^@chromaglyph\/(node|cli)-/.test(k)) j.optionalDependencies[k] = PKG_VER;
  }
}
fs.writeFileSync(PKG_FILE, JSON.stringify(j, null, 2) + "\n");
EOF
}

FOLDED=0
fold_changelog() {
  if ! grep -q '^## Unreleased$' CHANGELOG.md; then
    echo "  ⚠ CHANGELOG.md 没有 '## Unreleased' 段,跳过折叠" >&2
    return 0
  fi
  if ! awk '/^## Unreleased$/{f=1;next} /^## /{f=0} f && NF{ok=1} END{exit !ok}' CHANGELOG.md; then
    echo "  ⚠ '## Unreleased' 段是空的,跳过折叠" >&2
    return 0
  fi
  local DATE; DATE=$(date +%Y-%m-%d)
  FOLDED=1
  edit "CHANGELOG: Unreleased -> $NEW ($DATE)" \
    env DATE="$DATE" perl -0pi -e '
      s{^## Unreleased$}{## $ENV{NEW} ($ENV{DATE})}m;
      s{\A# Changelog\n}{# Changelog\n\n## Unreleased\n}' CHANGELOG.md
}

# ---------------------------------------------------------------- 执行

if [ "$DRY" -eq 1 ]; then echo "== dry-run =="; fi

if [ "$JS" -eq 1 ]; then
  echo "JS 线: @chromaglyph/$(echo $JS_PKGS | tr ' ' ',')  $BASE -> $NEW"
  for p in $JS_PKGS; do pkg_set_version "js/$p/package.json"; done
  # 不跑 pnpm install:js 包自身版本不进 lockfile,跑 install 反而会把
  # npm 上已发布的平台包 optionalDependencies 拉进 lockfile 并自动改写
  # pnpm-workspace.yaml(minimumReleaseAgeExclude),与 js 发版无关
else
  echo "原生线: $BASE -> $NEW"

  edit "workspace 版本 -> $NEW: Cargo.toml" \
    perl -pi -e 's{^version = "[^"]+"$}{version = "$ENV{NEW}"}' Cargo.toml
  edit "chromaglyph-core 依赖要求 -> $NEW: Cargo.toml" \
    perl -pi -e 's!^(chromaglyph-core = \{ [^}]*?version = ")[^"]+(")!${1}$ENV{NEW}${2}!' Cargo.toml
  for t in crates/chromaglyph/Cargo.toml crates/chromaglyph-node/Cargo.toml; do
    edit "chromaglyph-treesitter 依赖要求 -> $NEW: $t" \
      perl -pi -e 's!^(chromaglyph-treesitter = \{ [^}]*?version = ")[^"]+(")!${1}$ENV{NEW}${2}!' "$t"
  done

  pkg_set_version crates/chromaglyph-node/package.json 1
  pkg_set_version js/cli/package.json 1
  for p in npm/@chromaglyph/*/package.json; do
    pkg_set_version "$p"
  done

  fold_changelog

  edit "再生成 Cargo.lock(仅 workspace 成员版本)" cargo update --workspace
  edit "再生成 pnpm-lock.yaml" pnpm install --lockfile-only
fi

# ---------------------------------------------------------------- 校验与收尾

if [ "$DRY" -eq 0 ]; then
  if [ "$JS" -eq 1 ]; then
    VERIFY_FILES=""
    for p in $JS_PKGS; do VERIFY_FILES="$VERIFY_FILES js/$p/package.json"; done
  else
    VERIFY_FILES="Cargo.toml crates/chromaglyph/Cargo.toml crates/chromaglyph-node/Cargo.toml \
crates/chromaglyph-node/package.json js/cli/package.json \
npm/@chromaglyph/*/package.json"
    [ "$FOLDED" -eq 1 ] && VERIFY_FILES="$VERIFY_FILES CHANGELOG.md"
  fi
  MISS=0
  for f in $VERIFY_FILES; do
    grep -q "$NEW" "$f" || { echo "  ✗ $f 中没找到 $NEW" >&2; MISS=1; }
  done
  [ "$MISS" -eq 0 ] || die "校验失败:部分文件未更新到 ${NEW},请检查 git diff"

  git --no-pager diff --stat

  if [ "$TAG" -eq 1 ]; then
    git add Cargo.toml Cargo.lock pnpm-lock.yaml CHANGELOG.md \
           crates/chromaglyph/Cargo.toml crates/chromaglyph-node \
           js/cli npm/@chromaglyph
    git commit -m "release $NEW${DESC:+: $DESC}"
    git tag -a "v$NEW" -m "release $NEW"
    cat <<EOF

已提交并打 tag v${NEW}。push 将触发公开发布(npm 全套 + GitHub Release 二进制):
  git push origin main v${NEW}
确认无误后再推。
EOF
  elif [ "$JS" -eq 1 ]; then
    echo "完成。手动提交;js 线单独发版需手动 publish:"
    echo "  pnpm --filter @chromaglyph/remark publish --access public --no-git-checks"
    echo "或留着随下一次原生线 v* tag 由 CI 一起发布"
  else
    echo "完成。检查 git diff 后,提交并打 tag: git tag -a v${NEW} -m 'release ${NEW}'"
  fi
fi
