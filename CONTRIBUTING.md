# Contributing to CyMesh

To test a relay build, follow the [developer build guide](README.md#relay-builder). To add a running relay from your desktop client, follow the [volunteer guide](README.md#grow-the-mesh). When reporting a connection issue, include the route you used and the message status you saw.

1. Follow the setup in [README.md](README.md).
2. Describe the affected platform and user-visible behavior in your issue or pull request. For a networking issue, describe the test topology and use test identities.
3. Run `cargo test --locked -p aegis-core` for Rust changes and `npm --prefix ui run build` for UI changes.
4. When changing visible text, update the English and Persian dictionaries and verify the LTR and RTL layouts.

Use synthetic test data and reserved example addresses in fixtures. Keep local configuration, credentials, vaults and compiled outputs outside commits. See [SECURITY.md](SECURITY.md) for private security reports.

## فارسی

<div dir="rtl" lang="fa">

برای آزمایش نسخهٔ سفارشی از [راهنمای بیلدر](README.fa.md#relay-builder) و برای افزودن رله از داخل برنامه از [راهنمای داوطلب‌ها](README.fa.md#grow-the-mesh) استفاده کنید. در گزارش مشکل اتصال، مسیر اتصال و وضعیتی را که برای پیام دیدید بنویسید.

1. برای آماده کردن محیط توسعه، [راهنمای فارسی ساخت](README.fa.md#ساخت-از-روی-سورس) را دنبال کنید.
2. در گزارش یا درخواست ادغام، سیستم‌عامل و رفتار قابل مشاهده را بنویسید. اگر مشکل شبکه است، توضیح دهید دستگاه‌ها چطور به هم وصل بودند و از هویت‌های آزمایشی استفاده کنید.
3. برای تغییر در هسته، `cargo test --locked -p aegis-core` را اجرا کنید. برای تغییر رابط، `npm --prefix ui run build` را اجرا کنید.
4. اگر متن رابط تغییر کرد، هر دو ترجمه را به‌روز کنید و چیدمان فارسی و انگلیسی را بررسی کنید.

در نمونه‌ها از دادهٔ آزمایشی و آدرس‌های رزروشده استفاده کنید. تنظیمات محلی، رمزها، فایل صندوق و خروجی بیلد را خارج از کامیت نگه دارید. [گزارش خصوصی مشکل امنیتی](SECURITY.md#فارسی)

</div>
