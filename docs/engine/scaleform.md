# The Scaleform runtime

The Scaleform build the game links, the loader every movie comes through, how files reach it, the standalone movies outside the root movie, and what the shipped movies offer other movies. The screen framework and the traffic between ActionScript and native code are in [pause-menu.md](pause-menu.md#screens-and-the-root-movie); the per-frame capture and draw are in [rendering/ui-rendering.md](rendering/ui-rendering.md). The bindings are in [`ui::scaleform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::scaleform). Statements marked as resting on Scaleform's source are Scaleform 4 behaviour read from its source code, not verified in the IDB.

## Version and language

Every shipped movie carries an ExporterInfo tag from Scaleform's 4.1 exporter, with SWF version 12 (`root`, `dyn_root`) or 15 (all others). The binary holds no version string, but its runtime has classes newer than 4.1 (`AppLifecycleEvent`, `AccelerometerEvent`, `ASIMEManager`), so it is a later GFx 4.x build, inferred to be of the 4.2 to 4.4 era. The renderer is the D3D1x HAL, [`RenderHAL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::RenderHAL).

Every movie's FileAttributes tag sets the AS3 flag. The UI manager installs AS3 support and no AS2 support, so AS2 content does not run although AS2 code is linked (unverified). Application domains are disabled ([pause-menu.md](pause-menu.md#screens-and-the-root-movie)).

## The loader

`CUIManager::InitializeSystem` builds one `GFx::Loader`, [`UIManager::m_Loader`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_Loader), and installs its states: zlib support, the file opener [`ScaleformFileOpener`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ScaleformFileOpener), AS3 support, an FSCommand handler that only logs an error, the ExternalInterface handler [`ScaleformEIHandler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ScaleformEIHandler), a font map, a font library, the translator `CScaleformTranslator`, a threaded task manager, and a DDS image handler (the list of states is unverified). It then loads the fonts, loads `ui/root.gfx` with [`Loader::CreateMovie`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::Loader##func-CreateMovie), creates the root movie instance, and initialises every registered screen. Every movie created through `m_Loader` shares its states.

Per Scaleform's source (`SWFProcessInfo`, `MovieBindProcess`), the loader accepts the signatures `FWS`, `CWS`, `GFX`, and `CFX`, not the LZMA `ZWS`. `GFX` and `CFX` mark the movie as stripped, and a stripped movie's import URL ending in `.swf` is retried as `.gfx` relative to the movie's path, which is how the shipped movies' import of `shared_lib.swf` loads `ui/shared_lib.gfx`. A plain `FWS` or `CWS` movie's imports are resolved literally.

## Files

[`ScaleformFileOpener::OpenFile`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ScaleformFileOpener##func-OpenFile) serves every file a movie loads: the UI manager's file-cache entry when there is one, otherwise a `CScaleformCachedFile` read from the game's virtual file system and offered to the cache. The cache keeps only the movies on a fixed list built in its constructor (among them `ui/pause.gfx`, `ui/comm_link.gfx`, and `ui/dyn_root.gfx`; the full list is unverified). The engine builds a `Scaleform::MemoryFile` elsewhere, for its avatar fallback.

The virtual file system mounts each `--vfs-fs` and `--vfs-archive` path from the command line in order, each at a higher priority than the last. A path containing `dropzone` is mounted under that name, the convention of the community's loose-file dropzone folders, and `DestroyVfs` unmounts `dropzone`. The option strings and the unmount are in the IDB; the parsing is unverified.

The Gibbed file lists name 31 `ui/*.gfx` movies, all `CFX`. The patch archives hold copies of `root`, `shared_lib`, `pause`, `hud`, `overlay`, the `comm_*` screens, the `dyn_*` movies, and most fonts, and the game loads the patch copy where one exists; `dyn_root` is only in the base archives. Extracting and decompiling them is in [pause-menu.md](pause-menu.md#reading-the-movies).

## Standalone movies: CRenderToTextureUI

The in-world screens, the `CDynUI` game objects (the front-end start screen, the ROM posters, the video players), each own a separate movie, a `CRenderToTextureUI`, rather than a clip in the root movie. None of its members is bound; the sequence is verified in the IDB.

- `CRenderToTextureUI::InitMovie` loads `ui/dyn_root.gfx` through `m_Loader` with the root movie's load flags, creates an instance with the UI thread's command queue, gives it one mouse cursor and one controller, sets its view alignment, makes the main thread its capture thread, and advances it once.
- `InitCodeInterface` attaches a non-static screen object (`CDefaultDynUI`, `CFrontEndUI`, and others) as the movie's user data, so its ExternalInterface calls reach that screen. `CRenderToTextureUI::Activate` calls `root.ActivateDyn(<movie name>)`, and `dyn_root.gfx` loads any `<name>.gfx` with no table lookup, unlike `root.gfx`'s `Activate`.
- `InitRenderTarget` creates a colour texture, a D24S8 depth texture, and a render setup, and sets the movie viewport to the texture size.
- `UIManager::AddRenderToTextureUI` puts it on the manager's render-to-texture list, from which the manager advances and draws it ([ui-rendering.md](rendering/ui-rendering.md#off-screen-movies)) and releases it through the dying-movie list.

A movie that is not a registered screen has no input priority: [`UIManager::PreUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-PreUpdate) dispatches input only to the highest-priority active registered screen, and [`SendMouseEvents`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-SendMouseEvents) feeds only the root movie. A new static screen in the root movie would need an entry in `root.gfx`'s class-to-file table, hard-coded in its ActionScript.

## Native access to a movie

[`MovieImpl::CreateFunction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##func-CreateFunction) wraps a native handler in an AS3 function object, adds a reference to the handler, and keeps the user data beside it. Per Scaleform's source, the handler is a reference-counted object whose one virtual besides the destructor, `Call`, receives a parameter block of the return value, the movie, `this`, the arguments and their count, and the user data. Because [`ScaleformEIHandler::Callback`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ScaleformEIHandler##func-Callback) drops calls from a movie with no user data and an unknown prefix, such a function object is the only way that movie can call native code without a detour.

`CreateObject` with a class name instantiates any class in the shared domain, display classes included, so native code can build display objects and add them with an `addChild` invoke. `AttachMovie`, `CreateEmptyMovieClip`, and `GotoAndPlay` are on the value object interface by name, match the GFx 4 API, and are not bound. The rest of the value API is in [pause-menu.md](pause-menu.md#calls-from-native-code-to-the-movie).

## Fonts, widgets, and localisation

`ui/font_en.gfx` defines `FranklinGothicMediumITC` and `FranklinGothicHeavyITC`, exported as `$MainFont` and `$SecondaryFont`; `font_ara`, `font_jp`, `font_pl`, and `font_ru` are the per-language variants. `CUIManager::LoadFont` loads the language's font movie into the loader's font library, and the font map is filled from a language map file. Per Scaleform's source, an import of the font library's name resolves from the library instead of a file. Every game screen imports `font_en.swf` with no symbols, so a movie loaded through `m_Loader` that does the same and uses `$MainFont` or `$SecondaryFont` should get the game's fonts in every language (untested).

`ui/shared_lib.gfx` is the shared widget library, with several hundred SymbolClass entries: `shared.menu`, `shared.menu_button`, `shared.generic_button`, `shared.button_hint_bar` and `button_hint_item`, the scrollbar managers, `shared.checkbox_manager`, `shared.picker`, `shared.progress_bar`, the message box `shared.msg.c_msgbox_base`, the context menu and filter packages, the backgrounds and title classes, and every controller button glyph. Screens bring it in with a symbol-less `ImportAssets2` tag for `shared_lib.swf`. The settings slider `pause_code.slider` is not in it but in `pause.gfx`, which exports nothing ([pause-menu.md](pause-menu.md#row-objects-and-widgets)). `root.gfx`'s `uibase` class holds the palette and text helpers (`COLOR_TEXT_SELECTED`, `COLOR_TEXT_DEFAULT`, the `TM_*` tween modes, `SetTextFieldText`, `SetTextAutoSize`).

`CScaleformTranslator::Translate` rewrites a text field's text when it is set: text beginning with `&` is looked up as a string key (such as `&settings_yes`), text beginning with `#` and digits by hash, and anything else passes through, except that in keyboard mode action-key placeholders are substituted. `TextFieldEx.setNoTranslate` opts a field out. The class is in the IDB; its behaviour is unverified. String lookup itself is in [button-hints-and-localisation.md](button-hints-and-localisation.md).

## Building a movie the loader accepts

Scaleform's exporter is Windows-only and proprietary, but the Apache Flex SDK's `mxmlc` (nixpkgs `apache-flex-sdk`) builds the same shape on Linux: a `CWS` SWF with an AS3 FileAttributes tag, a `DoABC` tag, and a SymbolClass tag, given `-swf-version=15 -static-link-runtime-shared-libraries=true` and the stage size in `-default-size`. Its only `playerglobal.swc` targets a Flash Player whose APIs are a superset of Scaleform's, so the compiler does not catch calls Scaleform lacks; staying within `flash.display`, `flash.text`, `flash.events`, and the `scaleform.gfx.*` extensions avoids them. Haxe's SWF target, which can type against an existing library's classes, is untried.

Two post-build steps make it behave like a shipped movie: renaming the signature from `CWS` to `CFX`, so the loader treats it as stripped and resolves its `.swf` imports to the shipped `.gfx` files, and appending symbol-less `ImportAssets2` tags for `font_en.swf` and `shared_lib.swf`, which `mxmlc` does not emit (JPEXS FFDec can add them, as can a tool reusing the tag parser in `tools/scaleform-gfx-examine`).

None of this has been loaded in the game. Open: whether a Flex-built movie runs cleanly under Scaleform's AVM2, whether it needs an ExporterInfo tag (the loader appears to key the stripped flag off the signature alone), whether the `font_en` and `shared_lib` imports resolve for a movie outside the game's set, and whether `shared.*` classes work without `root.gfx`'s `uibase` context.
