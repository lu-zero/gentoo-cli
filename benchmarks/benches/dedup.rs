use std::collections::HashSet;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use portage_atom::DepEntry;
use portage_metadata::{LicenseExpr, RequiredUseExpr};

// ── Representative dep strings ─────────────────────────────────────────────
//
// ffmpeg is a good proxy: ~80 top-level entries, many USE conditionals,
// no pre-existing duplicates.  "doubled" simulates an eclass appending
// ${RDEPEND} after the ebuild already expanded it — the common source of
// duplicate entries in the wild.

const FFMPEG_RDEPEND: &str = "\
virtual/libiconv\n\
X? ( x11-libs/libX11 x11-libs/libXext x11-libs/libXv x11-libs/libxcb )\n\
alsa? ( media-libs/alsa-lib )\n\
amr? ( media-libs/opencore-amr )\n\
amrenc? ( media-libs/vo-amrwbenc )\n\
bluray? ( media-libs/libbluray )\n\
bs2b? ( media-libs/libbs2b )\n\
bzip2? ( app-arch/bzip2 )\n\
cdio? ( dev-libs/libcdio-paranoia )\n\
chromaprint? ( media-libs/chromaprint )\n\
codec2? ( media-libs/codec2 )\n\
dav1d? ( media-libs/dav1d )\n\
drm? ( x11-libs/libdrm )\n\
fdk? ( media-libs/fdk-aac )\n\
flite? ( app-accessibility/flite )\n\
fontconfig? ( media-libs/fontconfig )\n\
frei0r? ( media-plugins/frei0r-plugins )\n\
fribidi? ( dev-libs/fribidi )\n\
gcrypt? ( dev-libs/libgcrypt )\n\
gme? ( media-libs/game-music-emu )\n\
gmp? ( dev-libs/gmp )\n\
gnutls? ( !openssl? ( net-libs/gnutls ) )\n\
gsm? ( media-sound/gsm )\n\
iec61883? ( media-libs/libiec61883 sys-libs/libavc1394 sys-libs/libraw1394 )\n\
ieee1394? ( media-libs/libdc1394 sys-libs/libraw1394 )\n\
jack? ( virtual/jack )\n\
jpeg2k? ( media-libs/openjpeg )\n\
kvazaar? ( media-libs/kvazaar )\n\
lame? ( media-sound/lame )\n\
libaom? ( media-libs/libaom )\n\
libaribb24? ( media-libs/aribb24 )\n\
libass? ( media-libs/libass )\n\
libcaca? ( media-libs/libcaca )\n\
libilbc? ( media-libs/libilbc )\n\
librtmp? ( media-video/rtmpdump )\n\
libsoxr? ( media-libs/soxr )\n\
lv2? ( media-libs/lilv media-libs/lv2 )\n\
lzma? ( app-arch/xz-utils )\n\
modplug? ( media-libs/libmodplug )\n\
ocr? ( app-text/tesseract )\n\
openal? ( media-libs/openal )\n\
opencl? ( virtual/opencl )\n\
opengl? ( media-libs/libglvnd )\n\
openh264? ( media-libs/openh264 )\n\
openmpt? ( media-libs/libopenmpt )\n\
openssl? ( >=dev-libs/openssl-3 )\n\
opus? ( media-libs/opus )\n\
pulseaudio? ( media-libs/libpulse )\n\
rabbitmq? ( net-libs/rabbitmq-c )\n\
rav1e? ( >=media-video/rav1e-0.4 )\n\
rubberband? ( media-libs/rubberband )\n\
samba? ( net-fs/samba )\n\
sdl? ( media-libs/libsdl2 )\n\
snappy? ( app-arch/snappy )\n\
sndio? ( media-sound/sndio )\n\
speex? ( media-libs/speex )\n\
srt? ( net-libs/srt )\n\
ssh? ( net-libs/libssh )\n\
svg? ( dev-libs/glib >=gnome-base/librsvg-2.52 x11-libs/cairo )\n\
svt-av1? ( >=media-libs/svt-av1-0.8.4 )\n\
theora? ( media-libs/libtheora )\n\
truetype? ( media-libs/freetype )\n\
twolame? ( media-sound/twolame )\n\
v4l? ( media-libs/libv4l )\n\
vaapi? ( media-libs/libva )\n\
vdpau? ( x11-libs/libX11 x11-libs/libvdpau )\n\
vidstab? ( media-libs/vidstab )\n\
vorbis? ( media-libs/libvorbis )\n\
vpx? ( media-libs/libvpx )\n\
vulkan? ( media-libs/vulkan-loader )\n\
webp? ( media-libs/libwebp )\n\
x264? ( media-libs/x264 )\n\
x265? ( media-libs/x265 )\n\
xml? ( dev-libs/libxml2 )\n\
xvid? ( media-libs/xvid )";

// Simulates an eclass setting RDEPEND="${RDEPEND} ..." where both ebuild
// and eclass expand the same content — every entry appears exactly twice.
fn doubled(s: &str) -> String {
    format!("{s}\n{s}")
}

// Mirrors the logic in EbuildMetadata::dedup() / dedup_dep().
fn dedup_dep(entries: Vec<DepEntry>) -> Vec<DepEntry> {
    let mut seen: HashSet<DepEntry> = HashSet::new();
    let mut result = Vec::with_capacity(entries.len());
    for e in entries {
        if seen.insert(e.clone()) {
            result.push(e);
        }
    }
    result
}

// ── Dep benchmarks ─────────────────────────────────────────────────────────

fn bench_dep_dedup(c: &mut Criterion) {
    let ffmpeg_2x = doubled(FFMPEG_RDEPEND);

    let ffmpeg_entries = DepEntry::parse(FFMPEG_RDEPEND).unwrap();
    let ffmpeg_2x_entries = DepEntry::parse(&ffmpeg_2x).unwrap();

    let mut g = c.benchmark_group("dedup/dep");

    // ── no duplicates ──────────────────────────────────────────────────────
    g.bench_function("ffmpeg: parse", |b| {
        b.iter(|| black_box(DepEntry::parse(black_box(FFMPEG_RDEPEND)).unwrap()))
    });
    g.bench_function("ffmpeg: parse+dedup", |b| {
        b.iter(|| {
            let e = DepEntry::parse(black_box(FFMPEG_RDEPEND)).unwrap();
            black_box(dedup_dep(e))
        })
    });
    g.bench_function("ffmpeg: dedup only", |b| {
        b.iter(|| black_box(dedup_dep(ffmpeg_entries.clone())))
    });

    // ── all entries duplicated (worst case) ───────────────────────────────
    g.bench_function("ffmpeg(2x): parse", |b| {
        b.iter(|| black_box(DepEntry::parse(black_box(&ffmpeg_2x)).unwrap()))
    });
    g.bench_function("ffmpeg(2x): parse+dedup", |b| {
        b.iter(|| {
            let e = DepEntry::parse(black_box(&ffmpeg_2x)).unwrap();
            black_box(dedup_dep(e))
        })
    });
    g.bench_function("ffmpeg(2x): dedup only", |b| {
        b.iter(|| black_box(dedup_dep(ffmpeg_2x_entries.clone())))
    });

    g.finish();
}

// ── License benchmarks ─────────────────────────────────────────────────────

const FFMPEG_LICENSE: &str =
    "GPL-2 amr? ( GPL-3 ) gpl? ( GPL-3 ) openssl? ( openssl ) fdk? ( openssl )";

fn bench_license_dedup(c: &mut Criterion) {
    let doubled_lic = doubled(FFMPEG_LICENSE);

    let lic = LicenseExpr::parse(FFMPEG_LICENSE).unwrap();
    let lic_2x = LicenseExpr::parse(&doubled_lic).unwrap();

    let mut g = c.benchmark_group("dedup/license");

    g.bench_function("ffmpeg: parse", |b| {
        b.iter(|| black_box(LicenseExpr::parse(black_box(FFMPEG_LICENSE)).unwrap()))
    });
    g.bench_function("ffmpeg: parse+dedup", |b| {
        b.iter(|| {
            let l = LicenseExpr::parse(black_box(FFMPEG_LICENSE)).unwrap();
            black_box(l.dedup())
        })
    });
    g.bench_function("ffmpeg: dedup only", |b| b.iter(|| black_box(lic.dedup())));

    g.bench_function("ffmpeg(2x): parse", |b| {
        b.iter(|| black_box(LicenseExpr::parse(black_box(&doubled_lic)).unwrap()))
    });
    g.bench_function("ffmpeg(2x): parse+dedup", |b| {
        b.iter(|| {
            let l = LicenseExpr::parse(black_box(&doubled_lic)).unwrap();
            black_box(l.dedup())
        })
    });
    g.bench_function("ffmpeg(2x): dedup only", |b| {
        b.iter(|| black_box(lic_2x.dedup()))
    });

    g.finish();
}

// ── Required-use benchmarks ────────────────────────────────────────────────

const FFMPEG_REQUIRED_USE: &str = "amr? ( gpl ) amrenc? ( gpl ) codec2? ( gpl ) fdk? ( !gpl ) gme? ( gpl ) \
     openssl? ( !gpl ) rav1e? ( !gpl ) samba? ( gpl ) vidstab? ( gpl ) \
     x264? ( gpl ) x265? ( gpl ) xvid? ( gpl ) \
     cpu_flags_x86_mmx? ( cpu_flags_x86_mmxext )";

fn bench_required_use_dedup(c: &mut Criterion) {
    let doubled_ru = doubled(FFMPEG_REQUIRED_USE);

    let ru = RequiredUseExpr::parse(FFMPEG_REQUIRED_USE).unwrap();
    let ru_2x = RequiredUseExpr::parse(&doubled_ru).unwrap();

    let mut g = c.benchmark_group("dedup/required_use");

    g.bench_function("ffmpeg: parse", |b| {
        b.iter(|| black_box(RequiredUseExpr::parse(black_box(FFMPEG_REQUIRED_USE)).unwrap()))
    });
    g.bench_function("ffmpeg: parse+dedup", |b| {
        b.iter(|| {
            let r = RequiredUseExpr::parse(black_box(FFMPEG_REQUIRED_USE)).unwrap();
            black_box(r.dedup())
        })
    });
    g.bench_function("ffmpeg: dedup only", |b| b.iter(|| black_box(ru.dedup())));

    g.bench_function("ffmpeg(2x): parse", |b| {
        b.iter(|| black_box(RequiredUseExpr::parse(black_box(&doubled_ru)).unwrap()))
    });
    g.bench_function("ffmpeg(2x): parse+dedup", |b| {
        b.iter(|| {
            let r = RequiredUseExpr::parse(black_box(&doubled_ru)).unwrap();
            black_box(r.dedup())
        })
    });
    g.bench_function("ffmpeg(2x): dedup only", |b| {
        b.iter(|| black_box(ru_2x.dedup()))
    });

    g.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(200);
    targets = bench_dep_dedup, bench_license_dedup, bench_required_use_dedup
);
criterion_main!(benches);
