const fs = require("fs");
const path = require("path");

const repoRoot = path.resolve(__dirname, "..");
const distDir = path.join(repoRoot, "dist");

if (!fs.existsSync(distDir)) {
    throw new Error(`dist directory not found: ${distDir}`);
}

function maybeAddTarget(targets, targetDir) {
    if (fs.existsSync(targetDir)) {
        targets.push(targetDir);
    }
}

function maybeAddFileTarget(targets, targetFile) {
    if (fs.existsSync(path.dirname(targetFile))) {
        targets.push(targetFile);
    }
}

function shouldSyncTemplateBinaries() {
    return process.env.ML_SYNC_TEMPLATE_BINARIES === "1";
}

function shouldSyncTemplateStatic() {
    return process.env.ML_SYNC_TEMPLATE_STATIC !== "0";
}

function getRuntimeStaticTargets() {
    const targets = [];

    maybeAddTarget(targets, path.join(repoRoot, "runtime", "moonlight", "static"));

    const exportDir = path.join(repoRoot, "export");
    if (fs.existsSync(exportDir)) {
        for (const entry of fs.readdirSync(exportDir, { withFileTypes: true })) {
            if (!entry.isDirectory()) {
                continue;
            }

            if (entry.name.startsWith("_")) {
                continue;
            }

            if (entry.name.endsWith("-template") && !shouldSyncTemplateStatic()) {
                continue;
            }

            maybeAddTarget(
                targets,
                path.join(exportDir, entry.name, "moonlight", "static"),
            );
        }
    }

    return [...new Set(targets)];
}

function getDynamicDisplayHelperTargets() {
    const targets = [];
    const helperRelativePaths = [
        path.join("moonlight", "server", "display-prepare-helper.exe"),
        path.join("moonlight", "server", "display-prepare-helper.dll"),
        path.join("moonlight", "server", "display-prepare-helper.deps.json"),
        path.join("moonlight", "server", "display-prepare-helper.runtimeconfig.json"),
    ];

    for (const helperRelativePath of helperRelativePaths) {
        maybeAddFileTarget(
            targets,
            path.join(repoRoot, "runtime", helperRelativePath),
        );

        const exportDir = path.join(repoRoot, "export");
        if (fs.existsSync(exportDir)) {
            for (const entry of fs.readdirSync(exportDir, { withFileTypes: true })) {
                if (!entry.isDirectory()) {
                    continue;
                }

                if (entry.name.startsWith("_") || entry.name.endsWith("-template")) {
                    continue;
                }

                maybeAddFileTarget(
                    targets,
                    path.join(exportDir, entry.name, helperRelativePath),
                );
            }
        }
    }

    return [...new Set(targets)];
}

function getSunshineRuntimeMetadataTargets() {
    const targets = [];
    const metadataRelativePaths = [
        path.join("sunshine", "sunshine_runtime_info.json"),
        path.join("sunshine-legacy", "sunshine_runtime_info.json"),
    ];

    for (const relativePath of metadataRelativePaths) {
        maybeAddFileTarget(targets, path.join(repoRoot, "runtime", relativePath));

        const exportDir = path.join(repoRoot, "export");
        if (!fs.existsSync(exportDir)) {
            continue;
        }

        for (const entry of fs.readdirSync(exportDir, { withFileTypes: true })) {
            if (!entry.isDirectory()) {
                continue;
            }

            if (entry.name.startsWith("_") || entry.name.endsWith("-template")) {
                continue;
            }

            maybeAddFileTarget(targets, path.join(exportDir, entry.name, relativePath));
        }
    }

    return [...new Set(targets)];
}

function getHostSupervisorTargets() {
    const targets = [];
    maybeAddFileTarget(
        targets,
        path.join(
            repoRoot,
            "runtime",
            "moonlight",
            "system",
            "cloudgime-runtime-agent.exe",
        ),
    );

    const exportDir = path.join(repoRoot, "export");
    if (fs.existsSync(exportDir)) {
        for (const entry of fs.readdirSync(exportDir, { withFileTypes: true })) {
            if (!entry.isDirectory()) {
                continue;
            }

            if (entry.name.endsWith("-template") && !shouldSyncTemplateBinaries()) {
                continue;
            }

            maybeAddFileTarget(
                targets,
                path.join(
                    exportDir,
                    entry.name,
                    "moonlight",
                    "system",
                    "cloudgime-runtime-agent.exe",
                ),
            );
        }
    }

    return [...new Set(targets)];
}

function getHostInstallerTargets() {
    const targets = [];
    maybeAddFileTarget(
        targets,
        path.join(repoRoot, "runtime", "moonlight", "host_installer.exe"),
    );

    const exportDir = path.join(repoRoot, "export");
    if (fs.existsSync(exportDir)) {
        for (const entry of fs.readdirSync(exportDir, { withFileTypes: true })) {
            if (!entry.isDirectory()) {
                continue;
            }

            if (entry.name.endsWith("-template") && !shouldSyncTemplateBinaries()) {
                continue;
            }

            maybeAddFileTarget(
                targets,
                path.join(repoRoot, "export", entry.name, "host-installer.exe"),
            );
        }
    }

    return [...new Set(targets)];
}

const targets = getRuntimeStaticTargets();
const helperTargets = getDynamicDisplayHelperTargets();
const runtimeMetadataTargets = getSunshineRuntimeMetadataTargets();
const hostSupervisorTargets = getHostSupervisorTargets();
const hostInstallerTargets = getHostInstallerTargets();
const helperPublishDir = path.join(
    repoRoot,
    "tools",
    "portable",
    "DisplayPrepareHelper",
    "bin",
    "Release",
    "net8.0-windows",
    "win-x64",
    "publish",
);
const helperRuntimeDir = path.join(repoRoot, "runtime", "moonlight", "server");
const helperSourceDir = fs.existsSync(path.join(helperPublishDir, "display-prepare-helper.exe"))
    ? helperPublishDir
    : helperRuntimeDir;
const helperEntries = [
    {
        source: path.join(helperSourceDir, "display-prepare-helper.exe"),
        targetName: "display-prepare-helper.exe",
        removeWhenMissing: false,
    },
    {
        source: path.join(helperSourceDir, "display-prepare-helper.dll"),
        targetName: "display-prepare-helper.dll",
        removeWhenMissing: helperSourceDir === helperPublishDir,
    },
    {
        source: path.join(helperSourceDir, "display-prepare-helper.deps.json"),
        targetName: "display-prepare-helper.deps.json",
        removeWhenMissing: helperSourceDir === helperPublishDir,
    },
    {
        source: path.join(helperSourceDir, "display-prepare-helper.runtimeconfig.json"),
        targetName: "display-prepare-helper.runtimeconfig.json",
        removeWhenMissing: helperSourceDir === helperPublishDir,
    },
];

if (targets.length === 0) {
    console.log("[sync-runtime-static] no runtime static targets found");
}

const runtimeMetadataEntries = [
    {
        source: path.join(repoRoot, "runtime", "sunshine", "sunshine_runtime_info.json"),
        targetName: path.join("sunshine", "sunshine_runtime_info.json"),
    },
    {
        source: path.join(repoRoot, "runtime", "sunshine-legacy", "sunshine_runtime_info.json"),
        targetName: path.join("sunshine-legacy", "sunshine_runtime_info.json"),
    },
];

const hostSupervisorSourceCandidates = [
    path.join(repoRoot, "target", "release", "host_supervisor.exe"),
    path.join(repoRoot, "runtime", "moonlight", "host_supervisor.exe"),
];
const hostSupervisorSource = hostSupervisorSourceCandidates.find((candidate) =>
    fs.existsSync(candidate),
);
const hostInstallerSourceCandidates = [
    path.join(repoRoot, "target", "release", "host_installer.exe"),
    path.join(repoRoot, "runtime", "moonlight", "host_installer.exe"),
];
const hostInstallerSource = hostInstallerSourceCandidates.find((candidate) =>
    fs.existsSync(candidate),
);

for (const targetDir of targets) {
    fs.rmSync(targetDir, { recursive: true, force: true });
    fs.mkdirSync(path.dirname(targetDir), { recursive: true });
    fs.cpSync(distDir, targetDir, { recursive: true });

    console.log(
        `[sync-runtime-static] synced ${path.relative(repoRoot, targetDir)}`,
    );
}

for (const helperEntry of helperEntries) {
    const helperSource = helperEntry.source;
    const helperFileName = helperEntry.targetName;
    const matchingTargets = helperTargets.filter((target) => path.basename(target) === helperFileName);

    if (!fs.existsSync(helperSource)) {
        if (!helperEntry.removeWhenMissing) {
            continue;
        }

        for (const helperTarget of matchingTargets) {
            if (!fs.existsSync(helperTarget)) {
                continue;
            }
            try {
                fs.rmSync(helperTarget, { force: true });
                console.log(
                    `[sync-runtime-static] removed stale ${path.relative(repoRoot, helperTarget)}`,
                );
            } catch (error) {
                if (error && (error.code === "EBUSY" || error.code === "EPERM")) {
                    console.warn(
                        `[sync-runtime-static] skipped locked stale helper ${path.relative(repoRoot, helperTarget)} (${error.code})`,
                    );
                    continue;
                }
                throw error;
            }
        }
        continue;
    }

    for (const helperTarget of matchingTargets) {
        if (path.resolve(helperTarget) === path.resolve(helperSource)) {
            continue;
        }

        fs.mkdirSync(path.dirname(helperTarget), { recursive: true });
        try {
            fs.copyFileSync(helperSource, helperTarget);
            console.log(
                `[sync-runtime-static] synced ${path.relative(repoRoot, helperTarget)}`,
            );
        } catch (error) {
            if (error && (error.code === "EBUSY" || error.code === "EPERM")) {
                console.warn(
                    `[sync-runtime-static] skipped locked helper ${path.relative(repoRoot, helperTarget)} (${error.code})`,
                );
                continue;
            }
            throw error;
        }
    }
}

for (const metadataEntry of runtimeMetadataEntries) {
    const metadataSource = metadataEntry.source;
    if (!fs.existsSync(metadataSource)) {
        continue;
    }

    const matchingTargets = runtimeMetadataTargets.filter(
        (target) => path.relative(repoRoot, target).replaceAll("/", "\\").endsWith(metadataEntry.targetName),
    );

    for (const metadataTarget of matchingTargets) {
        if (path.resolve(metadataTarget) === path.resolve(metadataSource)) {
            continue;
        }

        fs.mkdirSync(path.dirname(metadataTarget), { recursive: true });
        fs.copyFileSync(metadataSource, metadataTarget);
        console.log(
            `[sync-runtime-static] synced ${path.relative(repoRoot, metadataTarget)}`,
        );
    }
}

if (hostSupervisorSource) {
    for (const target of hostSupervisorTargets) {
        if (path.resolve(target) === path.resolve(hostSupervisorSource)) {
            continue;
        }

        fs.mkdirSync(path.dirname(target), { recursive: true });
        try {
            fs.copyFileSync(hostSupervisorSource, target);
            console.log(
                `[sync-runtime-static] synced ${path.relative(repoRoot, target)}`,
            );
        } catch (error) {
            if (error && (error.code === "EBUSY" || error.code === "EPERM")) {
                console.warn(
                    `[sync-runtime-static] skipped locked host supervisor ${path.relative(repoRoot, target)} (${error.code})`,
                );
                continue;
            }
            throw error;
        }
    }
}

if (hostInstallerSource) {
    for (const target of hostInstallerTargets) {
        if (path.resolve(target) === path.resolve(hostInstallerSource)) {
            continue;
        }

        fs.mkdirSync(path.dirname(target), { recursive: true });
        try {
            fs.copyFileSync(hostInstallerSource, target);
            console.log(
                `[sync-runtime-static] synced ${path.relative(repoRoot, target)}`,
            );
        } catch (error) {
            if (error && (error.code === "EBUSY" || error.code === "EPERM")) {
                console.warn(
                    `[sync-runtime-static] skipped locked host installer ${path.relative(repoRoot, target)} (${error.code})`,
                );
                continue;
            }
            throw error;
        }
    }
}
