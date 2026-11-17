/** An imported SVG is its URL: Vite in the app, Bun's file loader in tests. */
declare module "*.svg" {
  const src: string;
  export default src;
}
