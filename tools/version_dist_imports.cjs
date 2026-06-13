const fs = require("fs");
const path = require("path");

function walkJsFiles(dir, files = []) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            walkJsFiles(fullPath, files);
        } else if (entry.isFile() && fullPath.endsWith(".js")) {
            files.push(fullPath);
        }
    }
    return files;
}

function extractBuildVersion(distDir) {
    const html = fs.readFileSync(path.join(distDir, "stream.html"), "utf8");
    const match = html.match(/stream\.js\?v=([^"&]+)/);
    if (!match) {
        throw new Error("failed to extract build version from dist/stream.html");
    }
    return match[1];
}

function versionSpecifier(specifier, version) {
    const [base] = specifier.split("?");
    if (!base.endsWith(".js")) {
        return specifier;
    }
    if (!base.startsWith("./") && !base.startsWith("../")) {
        return specifier;
    }
    return `${base}?v=${version}`;
}

function rewriteImports(source, version) {
    source = source.replace(
        /\b(import|export)\s+([^"'()]*?\s+from\s+)?(["'])(\.{1,2}\/[^"'"]+?\.js(?:\?[^"'"]*)?)\3/g,
        (match, keyword, middle = "", quote, specifier) =>
            `${keyword} ${middle ?? ""}${quote}${versionSpecifier(specifier, version)}${quote}`
    );

    source = source.replace(
        /\bimport\((["'])(\.{1,2}\/[^"'"]+?\.js(?:\?[^"'"]*)?)\1\)/g,
        (match, quote, specifier) => `import(${quote}${versionSpecifier(specifier, version)}${quote})`
    );

    source = source.replace(
        /\bnew URL\((["'])(\.{1,2}\/[^"'"]+?\.js(?:\?[^"'"]*)?)\1(\s*,\s*import\.meta\.url\s*\))/g,
        (match, quote, specifier, suffix) => `new URL(${quote}${versionSpecifier(specifier, version)}${quote}${suffix})`
    );

    return source;
}

function main() {
    const repoRoot = process.cwd();
    const distDir = path.join(repoRoot, "dist");
    const version = extractBuildVersion(distDir);
    const files = walkJsFiles(distDir);

    for (const filePath of files) {
        const before = fs.readFileSync(filePath, "utf8");
        const after = rewriteImports(before, version);
        if (after !== before) {
            fs.writeFileSync(filePath, after, "utf8");
        }
    }

    process.stdout.write(`Versioned dist module imports with ${version}\n`);
}

main();
