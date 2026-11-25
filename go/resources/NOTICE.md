# Bundled native library

`windows-amd64/ghostty-vt.dll` is the MyGo terminal plugin's Windows x64 build of
Ghostty's `libghostty-vt` from
`https://github.com/ghostty-org/ghostty/tree/befcdfd2c3a1cb24d9ec886e93c95b2b5daa7028`,
distributed by the MyGo project at
`https://github.com/egoist/mygo/releases/download/libghostty-vt-befcdfd2c3a1/libghostty-vt-windows-amd64.dll`.

- SHA-256: `53e8e2dd979702e8f2c48107af19be5a76efc2e88eb7cf1097218e899665da41`
- Used only by `github.com/egoist/mygo/plugins/terminal`; no cgo and no runtime download.
- Keep this notice and the upstream license terms in any package that ships the DLL.
