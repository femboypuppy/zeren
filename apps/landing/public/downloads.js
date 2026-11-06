(() => {
  const base = "https://github.com/femboypuppy/zeren/releases";
  const android = `${base}/latest`;
  const releases = {
    macos: ["macos-arm64.dmg", "Download for macOS", "Apple silicon"],
    windows: ["windows-x86_64-setup.exe", "Download for Windows", "Windows x64 · Installer"],
    "windows-portable": ["windows-x86_64.zip", "Download for Windows", "Windows x64 · Portable ZIP"],
    linux: ["linux-x86_64.tar.gz", "Download for Linux", "Linux x64"],
    "linux-arm": ["linux-aarch64.tar.gz", "Download for Linux", "Linux ARM64"],
  };
  const ua = navigator.userAgent || "";
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  const mobile = /Android|iPhone|iPad|iPod/i.test(ua) || navigator.userAgentData?.mobile
    || (/Mac/i.test(platform) && navigator.maxTouchPoints > 1);
  const onAndroid = /Android/i.test(ua) || /Android/i.test(platform);
  const os = mobile ? null : /Win/i.test(platform) ? "windows"
    : /Mac/i.test(platform) ? "macos" : /Linux/i.test(platform)
    ? (/aarch64|arm64/i.test(`${platform} ${ua}`) ? "linux-arm" : "linux") : null;
  // On an Android phone the buttons offer the APK (no version to wait for).
  if (onAndroid) {
    for (const id of ["nav-download", "hero-download", "closing-download"]) {
      const link = document.getElementById(id);
      if (!link) continue;
      link.href = android;
      link.textContent = id === "nav-download" ? "Downloads" : "View Android downloads";
      link.setAttribute("data-download-os", "android");
      link.setAttribute("aria-label", "View Android release assets");
      link.title = "View available Android builds";
    }
  }
  const apply = (release) => {
    const version = release.tag_name?.replace(/^v/, "");
    if (!/^\d+\.\d+\.\d+$/.test(version)) return;
    const assets = new Set(release.assets?.map((asset) => asset.name) || []);
    const download = (file) => {
      const name = `zeren-${version}-${file}`;
      return assets.has(name) ? `${base}/download/v${version}/${name}` : `${base}/latest`;
    };
    const apk = ["zeren-android-arm64.apk", `zeren-${version}-android-arm64.apk`].find(name => assets.has(name));
    if (onAndroid && apk) {
      for (const id of ["nav-download", "hero-download", "closing-download"]) {
        const link = document.getElementById(id);
        if (!link) continue;
        link.href = `${base}/download/v${version}/${apk}`;
        link.textContent = id === "nav-download" ? "Download" : "Download for Android";
        link.setAttribute("aria-label", "Download for Android (arm64 APK)");
        link.title = "arm64 APK";
      }
    }
    for (const link of document.querySelectorAll("[data-platform-download]")) {
      const release = releases[link.dataset.platformDownload];
      if (release) link.href = download(release[0]);
    }
    for (const id of ["nav-download", "hero-download", "closing-download"]) {
      const link = document.getElementById(id);
      if (!link || !os) continue;
      const [file, label, detail] = releases[os];
      link.href = download(file);
      link.textContent = id === "nav-download" ? "Download" : label;
      link.setAttribute("data-download-os", os);
      link.setAttribute("aria-label", `${label} (${detail})`);
      link.title = detail;
    }
  };
  fetch("https://api.github.com/repos/femboypuppy/zeren/releases/latest", { credentials: "omit" })
    .then((response) => response.ok ? response.json() : Promise.reject())
    .then(apply)
    .catch(() => {});
})();
