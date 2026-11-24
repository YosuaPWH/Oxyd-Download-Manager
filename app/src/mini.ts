import "./bootstrap";
import { mount } from "svelte";
import MiniWindow from "./MiniWindow.svelte";

mount(MiniWindow, { target: document.getElementById("app")! });
