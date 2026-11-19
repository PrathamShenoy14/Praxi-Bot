import { spawnSync } from "node:child_process";
import console from "node:console";
import { join } from "node:path";
import process from "node:process";
import { URL } from "node:url";

// Keep these minimums in sync with CONTRIBUTING.md.
const MINIMUM = { rust: "1.89.0", node: "24.0.0", pnpm: "10.0.0" };
const LINUX_PACKAGES = [
  "libwebkit2gtk-4.1-dev",
  "libappindicator3-dev",
  "librsvg2-dev",
  "patchelf",
  "libdbus-1-dev",
  "pkg-config",
  "libxcb1-dev",
];
const VERSION = /^(\d+)\.(\d+)\.(\d+)(-[\w.-]+)?(?:\+[\w.-]+)?$/;

function run(command, args, extraEnv = {}) {
  return spawnSync(command, args, {
    encoding: "utf8",
    env: { ...process.env, ...extraEnv },
    stdio: ["ignore", "pipe", "pipe"],
    timeout: 5000,
    windowsHide: true,
  });
}

function compareVersion(detected, minimum) {
  for (const [index, required] of minimum.split(".").entries()) {
    const difference = Number(detected[index + 1]) - Number(required);
    if (difference !== 0) return difference;
  }
  return detected[4] ? -1 : 0;
}

function checkVersion(name, version, minimum, hint) {
  const detected = VERSION.exec(version ?? "");
  const requirement = `requires ${minimum} or newer`;
  if (!detected) {
    console.log(`${name}: missing or unavailable (${requirement}). ${hint}`);
    return false;
  }
  const ok = compareVersion(detected, minimum) >= 0;
  console.log(`${name}: ${ok ? "ok" : "too old"} (${version}, ${requirement})${ok ? "" : `. ${hint}`}`);
  return ok;
}

function hasMsvc() {
  if (run("cl.exe", ["/?"]).status === 0) return true;

  const programFiles = process.env["ProgramFiles(x86)"] || process.env.ProgramFiles;
  if (!programFiles) return false;
  const vswhere = join(programFiles, "Microsoft Visual Studio", "Installer", "vswhere.exe");
  const result = run(vswhere, ["-products", "*", "-find", "VC\\Tools\\MSVC\\**\\cl.exe"]);
  return result.status === 0 && result.stdout.trim() !== "";
}

function checkCompiler() {
  let available;
  let name;
  let hint;
  if (process.platform === "win32") {
    available = hasMsvc();
    name = "MSVC";
    hint = "Install the Desktop development with C++ workload in Visual Studio Build Tools.";
  } else if (process.platform === "darwin") {
    // Running Apple's compiler shim directly can launch the developer tools installer.
    available = run("xcode-select", ["--print-path"]).status === 0;
    name = "Xcode command-line tools";
    hint = "Install the Xcode command-line tools with xcode-select --install.";
  } else {
    available = run("cc", ["--version"]).status === 0;
    name = "cc";
    hint = "Install a C toolchain, such as build-essential on Debian or Ubuntu.";
  }
  console.log(`C toolchain: ${available ? `ok (${name})` : `missing (${name}). ${hint}`}`);
  return available;
}

function checkAptPackages() {
  if (process.platform !== "linux") {
    console.log("Linux apt packages: skipped (Linux only).");
    return true;
  }

  let ok = true;
  for (const name of LINUX_PACKAGES) {
    const result = run("dpkg", ["-s", name], { LC_ALL: "C" });
    if (result.error?.code === "ENOENT") {
      console.log("Linux apt packages: skipped (dpkg is unavailable).");
      return ok;
    }
    const installed = result.status === 0 && /^Status: \S+ ok installed$/m.test(result.stdout);
    if (installed) {
      console.log(`${name}: ok (installed)`);
    } else {
      const status = result.error ? "could not check" : "missing";
      console.log(`${name}: ${status}. See the Linux apt install command in CONTRIBUTING.md.`);
      ok = false;
    }
  }
  return ok;
}

const rust = run("rustc", ["--version"], {
  RUSTUP_AUTO_INSTALL: "0",
  // Older Rustup versions can try to download the active toolchain even for --version.
  RUSTUP_DIST_SERVER: new URL("../rust-toolchain.toml", import.meta.url).href,
});
const rustVersion = rust.status === 0 ? /^rustc\s+(\S+)/.exec(rust.stdout)?.[1] : undefined;
const rustOk = checkVersion("Rust", rustVersion, MINIMUM.rust, "Install or update Rust with rustup (https://rustup.rs/).");

const nodeOk = checkVersion("Node.js", process.versions.node, MINIMUM.node, "Install Node.js from https://nodejs.org/download/.");

const pnpmEnv = {
  COREPACK_ENABLE_NETWORK: "0",
  COREPACK_DEFAULT_TO_LATEST: "0",
  COREPACK_ENABLE_AUTO_PIN: "0",
  npm_config_manage_package_manager_versions: "false",
};
const pnpm = process.platform === "win32"
  ? run(process.env.ComSpec || "cmd.exe", ["/d", "/c", "pnpm", "--version"], pnpmEnv)
  : run("pnpm", ["--version"], pnpmEnv);
const pnpmVersion = pnpm.status === 0 ? pnpm.stdout.trim() : undefined;
const pnpmOk = checkVersion("pnpm", pnpmVersion, MINIMUM.pnpm, "Install or update pnpm using https://pnpm.io/installation.");

const compilerOk = checkCompiler();
const aptOk = checkAptPackages();
process.exitCode = [rustOk, nodeOk, pnpmOk, compilerOk, aptOk].every(Boolean) ? 0 : 1;
