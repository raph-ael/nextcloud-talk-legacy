// electron-vite: `?asset` imports resolve to the file's path in the build output.
declare module '*?asset' {
  const path: string
  export default path
}
