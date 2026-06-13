const fs = require("fs");
const path = require("path");

const repoRoot = path.resolve(__dirname, "..");
const sourceCandidates = [
    path.join(
        repoRoot,
        "..",
        "android native app",
        "app",
        "src",
        "main",
        "assets",
        "native_bridge_panel.html",
    ),
    path.join(
        repoRoot,
        "android",
        "native_client_fork",
        "moonlight-android-upstream",
        "app",
        "src",
        "main",
        "assets",
        "native_bridge_panel.html",
    ),
];
const target = path.join(repoRoot, "dist", "native_bridge_panel.html");

const source = sourceCandidates.find((candidate) => fs.existsSync(candidate));

if (!source) {
    throw new Error(`native bridge panel source not found. checked: ${sourceCandidates.join(" | ")}`);
}

fs.mkdirSync(path.dirname(target), { recursive: true });
fs.copyFileSync(source, target);
process.stdout.write(`[copy-native-bridge] copied ${path.relative(repoRoot, source)} -> ${path.relative(repoRoot, target)}\n`);
