# OpenDyslexic

The font of the OpenDyslexic setting, under the SIL Open Font License in
OFL.txt, by Abbie Gonzalez.

- OpenDyslexic itself is not bundled any more: the apps download it from
  Fontsource the first time the setting is on (lib/dyslexicFont.ts).
- `opendyslexic-mono-400-normal.woff2` is OpenDyslexic Mono, which has no
  compiled release. It is built with fontmake from
  `sources/opendyslexic-mono/OpenDyslexic-Mono.glyphs` of
  github.com/antijingoist/opendyslexic, after removing 44 mark anchors with
  no name (`_`) that stop fontmake, then turned into woff2 with fontTools.
