import "./app.css";
import { mount } from "svelte";

// Load only this window's root and styles; a popup must not boot the main
// calendar's listeners, nor change its typography through global CSS.
async function start() {
  const { default: Root } = new URLSearchParams(location.search).has("menubar")
    ? await import("./lib/Menubar.svelte")
    : await import("./App.svelte");
  return mount(Root, { target: document.getElementById("app")! });
}
export default start();
