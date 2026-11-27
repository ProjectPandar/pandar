# pandar.css 的来源

`pandar.css` 不是手写的：它由仓库真实样式编译而来，所以画板上的类名与 `frontend/app/*.tsx`、`frontend/components/ui/*.tsx` 一致。

```bash
# 1. 临时目录安装与 frontend/package.json 相同版本的 Tailwind / shadcn（只需一次）
mkdir -p /tmp/twbuild && cd /tmp/twbuild && npm init -y
npm install tailwindcss@4.3.1 @tailwindcss/cli@4.3.1 shadcn@4.14.1 tw-animate-css@1.4.0

# 2. 输入 = frontend/app/globals.css + components.css（仅把仓库里反复出现的 class 串收拢成 .btn/.badge 等别名）+ @source
cat /home/indexyz/pandar/frontend/app/globals.css components.src.css > globals.css
printf '\n@source "/home/indexyz/pandar/frontend/app";\n@source "/home/indexyz/pandar/frontend/components";\n@source "/home/indexyz/pandar/design/pandar-hub-frontend";\n' >> globals.css

# 3. 编译到设计目录
npx @tailwindcss/cli -i globals.css -o /home/indexyz/pandar/design/pandar-hub-frontend/pandar.css --minify
```

画板新增了 Tailwind 类之后需要重新执行第 3 步，否则新类不会出现在 `pandar.css` 里。

`extra.css` 只做两件事：把仓库里 `fixed` 的侧边栏容器改成相对文档定位（画布按整页高度渲染），以及移动端画板的固定高度。
