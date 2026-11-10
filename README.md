# Gapir

Mac, Windows va Linux uchun oʻzbekcha ovozli yozuv. Istalgan matn maydonida `fn` tugmasini 1 soniya bosib turing va gapiring, aytganingiz kursor turgan joyga yoziladi. Ovoz shu kompyuterda matnga oʻgiriladi va hech qayerga yuborilmaydi.

## Oʻrnatish

Terminal'ni oching va shu buyruqni ishga tushiring:

```bash
curl -fsSL https://raw.githubusercontent.com/SalikhovID/gapir/main/install.sh | bash
```

Buyruq ilovani ham, taxminan 580 MB lik nutq modelini ham yuklaydi. Internet uzilsa, buyruqni qayta ishga tushiring: yuklash toʻxtagan joyidan davom etadi.

Birinchi ochilishda Gapir ikki ruxsat soʻraydi:

- **Mikrofon**: ovozingizni eshitish uchun.
- **Accessibility**: `fn` tugmasini sezish va matnni yozish uchun. System Settings ochiladi, roʻyxatda Gapir'ni yoqing.

Apple Silicon (M1 va undan yangi) va macOS 13 yoki undan yangisi kerak.

### Windows

[Oxirgi release](https://github.com/SalikhovID/gapir/releases/latest)dan `Gapir-windows-x64-setup.exe` ni yuklab, ishga tushiring. Installer imzolanmagan, shuning uchun SmartScreen ogohlantiradi: **More info** → **Run anyway**. Model birinchi ochilishda ilovaning oʻzida yuklanadi.

Windows'da `fn` oʻrniga **oʻng Ctrl** ishlaydi, `⌘⇧G` oʻrniga `Ctrl+Shift+G`. Windows 10 yoki 11 (x64) kerak. Nutq Vulkan orqali videokartada matnga oʻgiriladi (NVIDIA, AMD, Intel). Vulkan boʻlmasa, Gapir CPU'da ishlaydi, lekin sekinroq.

### Linux

[Oxirgi release](https://github.com/SalikhovID/gapir/releases/latest)dan `Gapir-linux-x86_64.AppImage` ni yuklab, ishga tushiring:

```bash
chmod +x Gapir-linux-x86_64.AppImage && ./Gapir-linux-x86_64.AppImage
```

Gapir tugmalarni eshitish va matnni yozish uchun klaviatura qurilmalariga kirishi kerak. Buni bir marta sozlaysiz: ilova oynasidagi **Buyruqni nusxalash** tugmasi shu buyruqni nusxalaydi, uni terminalda ishga tushiring va tizimdan chiqib qayta kiring:

```bash
echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/60-gapir.rules && sudo modprobe uinput && sudo udevadm control --reload && sudo udevadm trigger && sudo usermod -aG input $USER
```

Linux'da ham `fn` oʻrniga **oʻng Ctrl**, `⌘⇧G` oʻrniga `Ctrl+Shift+G` ishlaydi. Model birinchi ochilishda ilovaning oʻzida yuklanadi. Matn clipboard orqali `Shift+Insert` bilan qoʻyiladi, shuning uchun clipboard'da oxirgi boʻlak qoladi. Oʻng Ctrl bosib turilganda matn qoʻyib yuborganingizdan keyin tushadi. X11 va Wayland ishlaydi, x86_64 va PulseAudio yoki PipeWire kerak. Nutq CPU'da matnga oʻgiriladi.

## Ishlatish

| Tugma | Amal |
|---|---|
| `fn` 1 soniya bosib turish | Ekran pastida belgi chiqqach gapirasiz, qoʻyib yuborsangiz yozuv tugaydi |
| `⌘⇧G` | Bir bosish yozishni boshlaydi, yana bosish toʻxtatadi |
| `Esc` | Yozish paytida bosilsa, yozuv bekor boʻladi va yozilgan matn oʻchadi |

Matn gap orasidagi pauzalarda boʻlak-boʻlak tushadi, shuning uchun uzun matnni ham toʻxtamay aytishingiz mumkin. `⌘⇧G` bilan yozilganda 30 soniya jimlikdan keyin yozuv oʻzi tugaydi.

Boshqa oynaga oʻtsangiz, matn adashmaydi: Gapir eski oynaga qaytishingizni kutadi. Qaytmasangiz, matn menu bar'dagi **Oxirgi matnni nusxalash** bandida turadi.

Gapir fonda ishlaydi va menu bar'da turadi. `⌘Q` faqat oynani yopadi. Butunlay toʻxtatish uchun menu bar'dan **Butunlay chiqish**ni tanlang. Hotkey, mikrofon va avtomatik ishga tushish sozlamalari ham shu menyudagi **Sozlamalar**da.

## Bilib qoʻyish kerak

- `fn` faqat Apple klaviaturalarida ishlaydi. Boshqa klaviaturada `⌘⇧G` ishlating.
- `fn`ni qisqa bossangiz, Gapir ishlamaydi va mikrofon ochilmaydi. Qisqa bosish macOS'da qoladi, masalan layout almashtirish uchun.
- `⌘⇧G` Gapir ishlab turganda boshqa ilovalarga bormaydi, masalan Finder'dagi "Go to Folder". Kerak boʻlsa sozlamada boshqa birikma tanlang.
- Matn asosan kichik harfda keladi, Gapir faqat gap boshini katta qiladi. Raqamlar soʻz bilan yoziladi.
- Model oʻzbek tili uchun oʻqitilgan. Ruscha gap lotin harflarida chiqadi.
- Bir oyna ichida boshqa joyga oʻtish, masalan Telegram'da boshqa chatni ochish, oyna almashishi hisoblanmaydi: matn yangi joyga tushadi.

## Yangilash

Gapir har 5 daqiqada GitHub'dan oxirgi versiya raqamini soʻraydi, boshqa hech narsa yuborilmaydi. Yangi versiya chiqsa va siz gapirmayotgan boʻlsangiz, Gapir uni fonda yuklaydi, hash'ini tekshiradi, oʻrnatadi va oynasiz qayta ochiladi. Buni sozlamadagi **Avtomatik yangilash** bilan oʻchirish mumkin.

## Oʻchirish

```bash
curl -fsSL https://raw.githubusercontent.com/SalikhovID/gapir/main/uninstall.sh | bash
```

Ilova, model, sozlamalar, loglar va berilgan ruxsatlar oʻchiriladi.

## Dasturchilar uchun

Tauri 2 va Rust. Platformaga bogʻliq boʻlmagan mantiq `core/` da, Mac qismi `src-tauri/` da, oynalar `ui/` da.

```bash
cargo test --workspace
npx @tauri-apps/cli@2 build --bundles app
```

Build uchun Rust, CMake, Node va Command Line Tools kerak, Windows'da Visual Studio Build Tools va [Vulkan SDK](https://vulkan.lunarg.com/sdk/home#windows). Windows installer: `npx @tauri-apps/cli@2 build --bundles nsis`. Linux'da [CI](.github/workflows/ci.yml)dagi `LINUX_PACKAGES` kerak, AppImage: `npx @tauri-apps/cli@2 build --bundles appimage`.

Release avtomatik chiqadi. `main`ga har bir push yoki merge [CI](.github/workflows/ci.yml)ni ishga tushiradi: u fmt, clippy va testlarni macOS, Windows va Linux'da tekshiradi, patch versiyani bittaga oshiradi (`chore: release` commit va `vX.Y.Z` tegi), ilovani [`scripts/package.sh`](scripts/package.sh) bilan quradi, imzolaydi, Windows installer'ni va Linux AppImage'ni ham quradi va hammasini bitta GitHub release qilib yuklaydi. Release kerak boʻlmasa, commit xabariga `[skip ci]` yozing. Minor yoki major versiya uchun `scripts/set-version.sh 0.2.0` ni ishga tushirib, oʻzgarishni commit qiling: CI aynan shu versiyani chiqaradi. CI ishlamay qolsa, release qoʻlda ham chiqadi: `scripts/package.sh` zip yasaydi (`MACOS_CERT_P12` va `MACOS_CERT_PASSWORD` kerak), keyin `gh release create` bilan yuklanadi.

Model [`scripts/convert-model.py`](scripts/convert-model.py) bilan asl fayldan olinadi, aniq versiyalari [model release](https://github.com/SalikhovID/gapir/releases/tag/model-rubaistt-v2-q5km) izohida yozilgan.

## Litsenziya

Gapir kodi: [MIT](LICENSE).

Nutq modeli: [RubaiSTT v2 medium](https://huggingface.co/islomov/rubaistt_v2_medium), muallifi Sardor Islomov, [Apache-2.0](LICENSES/Apache-2.0.txt). Gapir shu modelning GGUF formatiga oʻtkazilgan va kvantlangan nusxasini tarqatadi, model qayta oʻqitilmagan.

Batafsil: [NOTICE](NOTICE) va [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
