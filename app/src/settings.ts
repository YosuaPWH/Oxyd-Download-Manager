import "./bootstrap";
import { mount } from "svelte";
import SettingsWindow from "./SettingsWindow.svelte";

mount(SettingsWindow, { target: document.getElementById("app")! });
