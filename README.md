# Gapir

Mac uchun oʻzbekcha ovozli yozuv. Istalgan matn maydonida `fn` tugmasini bosib turib gapiring, aytganingiz kursor turgan joyga yoziladi. Ovoz shu kompyuterda matnga oʻgiriladi va hech qayerga yuborilmaydi.

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

## Ishlatish

| Tugma | Amal |
|---|---|
| `fn` bosib turish | Gapirasiz, qoʻyib yuborsangiz yozuv tugaydi |
| `⌘⇧G` | Bir bosish yozishni boshlaydi, yana bosish toʻxtatadi |
| `Esc` | Yozish paytida bosilsa, yozuv bekor boʻladi va yozilgan matn oʻchadi |

Matn gap orasidagi pauzalarda boʻlak-boʻlak tushadi, shuning uchun uzun matnni ham toʻxtamay aytishingiz mumkin. `⌘⇧G` bilan yozilganda 30 soniya jimlikdan keyin yozuv oʻzi tugaydi.

Boshqa oynaga oʻtsangiz, matn adashmaydi: Gapir eski oynaga qaytishingizni kutadi. Qaytmasangiz, matn menu bar'dagi **Oxirgi matnni nusxalash** bandida turadi.

Gapir fonda ishlaydi va menu bar'da turadi. `⌘Q` faqat oynani yopadi. Butunlay toʻxtatish uchun menu bar'dan **Butunlay chiqish**ni tanlang. Hotkey, mikrofon va avtomatik ishga tushish sozlamalari ham shu menyudagi **Sozlamalar**da.

## Bilib qoʻyish kerak

- `fn` faqat Apple klaviaturalarida ishlaydi. Boshqa klaviaturada `⌘⇧G` ishlating.
- `fn` bosilganda layout almashsa yoki emoji oynasi ochilsa: System Settings → Keyboard → "Press 🌐 key to" → **Do Nothing**.
- `⌘⇧G` Gapir ishlab turganda boshqa ilovalarga bormaydi, masalan Finder'dagi "Go to Folder". Kerak boʻlsa sozlamada boshqa birikma tanlang.
- Matn asosan kichik harfda keladi, Gapir faqat gap boshini katta qiladi. Raqamlar soʻz bilan yoziladi.
- Model oʻzbek tili uchun oʻqitilgan. Ruscha gap lotin harflarida chiqadi.
- Bir oyna ichida boshqa joyga oʻtish, masalan Telegram'da boshqa chatni ochish, oyna almashishi hisoblanmaydi: matn yangi joyga tushadi.

## Yangilash

Yangi versiya chiqsa, menu bar menyusida **Yangilash** paydo boʻladi. Buning uchun Gapir kuniga bir marta GitHub'dan versiya raqamini soʻraydi, boshqa hech narsa yuborilmaydi. Tekshiruvni sozlamada oʻchirish mumkin.

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

Build uchun Rust, CMake, Node va Command Line Tools kerak. Linux va Windows hali yoʻq.

Release qoʻlda ham yigʻiladi: [`scripts/package.sh`](scripts/package.sh) ilovani quradi, imzolaydi va zip qiladi. Model [`convert-model`](.github/workflows/convert-model.yml) workflow'ida asl fayldan qayta yigʻiladi.

## Litsenziya

MIT. Nutq modeli [RubaiSTT v2](https://huggingface.co/islomov/rubaistt_v2_medium), muallifi Sardor Islomov, Apache-2.0. Batafsil: [NOTICE](NOTICE).
