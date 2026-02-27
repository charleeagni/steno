import { defineConfig } from "vite";

export default defineConfig({
    clearScreen: false,
    server: {
        port: 1433,
        strictPort: true,
    },
});
