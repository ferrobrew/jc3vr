//! The single-pass switch table: every single-pass production switch, declared once.
//!
//! Each row of the `single_pass_switches!` invocation below is one boolean switch, and the macro
//! generates everything that names the set: the [`SinglePassConfig`] fields and their defaults, the
//! [`SinglePassSwitch`] enum the capture overrides and the shader build key by, whether a change
//! needs the shaders re-created, the bit each switch takes in the render path's per-frame snapshot
//! ([`SinglePassConfig::switch_bits`]), the debug UI row, and the JSON manifest the Python capture
//! protocol (`tools/rendering/protocol`) reads. Adding a switch is adding a row.
//!
//! The master switch, [`SinglePassConfig::enabled`], is not a row: it is the mode itself, which the
//! capture modes set rather than override.

use serde::{Deserialize, Serialize};

macro_rules! single_pass_switches {
    ($(
        $(#[doc = $doc:literal])*
        $variant:ident => $field:ident {
            default: $default:literal,
            rebuilds: $rebuilds:literal,
            label: $label:literal,
            hover: $hover:literal,
            $(requires: $requires:ident,)?
        }
    )*) => {
        /// The single-pass stereo configuration: rendering both eyes in one geometry walk into a
        /// double-wide target.
        ///
        /// Its own struct because the feature is separable in both directions -- nothing here does
        /// anything with `enabled` off, and the double-draw path works with all of it inert.
        #[derive(Copy, Clone, PartialEq, Serialize, Deserialize)]
        pub struct SinglePassConfig {
            /// Master switch for single-pass stereo (see `docs/mod/stereo/single-pass-stereo.md`).
            /// Instead of the double-draw (two full `game.Draw` walks, one per eye), render the
            /// camera scene in one walk (the collapse) with the vertex shaders patched to emit both
            /// eyes via instancing, distinct per-eye `cb13` rows, and `SV_ViewportArrayIndex`
            /// routing into a render target twice an eye's width (the double-wide target, which
            /// needs [`vr.native_resolution`](crate::vr::VrConfig::native_resolution)). On by
            /// default, together with the rest of the group
            /// [`set_single_pass_feature`](crate::stereo::config::set_single_pass_feature)
            /// switches; the double-draw is what turning the group off leaves. Live only while a VR
            /// session runs (see `jc3vr_render::stereo::single_pass::active`), and requires the
            /// DXVK viewport-routing capability (see [`crate::stereo::layout::capability`]); forced
            /// off if absent.
            pub enabled: bool,
            $(
                $(#[doc = $doc])*
                pub $field: bool,
            )*
        }

        impl SinglePassConfig {
            pub const fn new() -> Self {
                Self {
                    enabled: true,
                    $($field: $default,)*
                }
            }

            /// Every switch's value as one bit at its [`SinglePassSwitch`] index, for the render
            /// path's per-frame snapshot.
            pub const fn switch_bits(&self) -> u64 {
                0 $(| ((self.$field as u64) << SinglePassSwitch::$variant as u64))*
            }
        }

        impl Default for SinglePassConfig {
            fn default() -> Self {
                Self::new()
            }
        }

        /// A single-pass production switch: one boolean field of [`SinglePassConfig`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SinglePassSwitch {
            $($variant,)*
        }

        impl SinglePassSwitch {
            /// Every switch, in table order.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The switch's configuration field name, which is also its name on the wire.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($field),)*
                }
            }

            /// The switch's value in a default configuration.
            pub const fn default_value(self) -> bool {
                match self {
                    $(Self::$variant => $default,)*
                }
            }

            /// Whether the switch is a rewrite applied when a shader is created, so a change only
            /// takes effect once the shaders are re-created.
            pub const fn rebuilds_shaders(self) -> bool {
                match self {
                    $(Self::$variant => $rebuilds,)*
                }
            }

            /// The debug UI row's label.
            pub const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)*
                }
            }

            /// The debug UI row's hover text.
            pub const fn hover(self) -> &'static str {
                match self {
                    $(Self::$variant => $hover,)*
                }
            }

            /// The switch this one only acts beneath, whose row the debug UI greys this one out
            /// under while it is off.
            pub const fn requires(self) -> Option<Self> {
                match self {
                    $(Self::$variant => single_pass_switches!(@requires $($requires)?),)*
                }
            }

            pub const fn get(self, config: &SinglePassConfig) -> bool {
                match self {
                    $(Self::$variant => config.$field,)*
                }
            }

            pub fn set(self, config: &mut SinglePassConfig, value: bool) {
                match self {
                    $(Self::$variant => config.$field = value,)*
                }
            }
        }
    };
    (@requires) => { None };
    (@requires $requires:ident) => { Some(Self::$requires) };
}

single_pass_switches! {
    /// Reproject the no-`cb0` scene-geometry families (skinned characters/NPCs, props, buildings,
    /// roads, ...) for single-pass, instead of leaving them double-drawn. When on, a vertex shader
    /// with no per-eye `cb0` operand whose name is on the reprojection allowlist is rewritten to
    /// post-multiply its own clip position by the per-eye `M_eye` (see
    /// `jc3vr_render::stereo::single_pass`); NDC writers (sky, UI, post) are excluded by the allowlist.
    /// Requires [`single_pass`](Self::enabled); independent of the others so it can be A/B'd.
    Reproject => reproject {
        default: true,
        rebuilds: true,
        label: "Reproject the no-cb0 scene families (NPCs, props, buildings, roads)",
        hover: "Rewrites the baked-WVP scene shaders (characters, props, buildings, roads) to \
                post-multiply their clip by the per-eye M_eye, instead of leaving them \
                double-drawn. Reloads shaders to apply. Sky/UI/post are excluded.",
    }
    /// Single-pass the tessellated base terrain (VS → HS → DS): the vertex shader originates the
    /// eye index on the free `TEXCOORD3.z` lane, the hull shader forwards it, and the domain shader
    /// reads it to reproject its clip by the per-eye `M_eye` and route to the eye's viewport.
    /// Covers the `DrawIndexed` terrain passes (far/color/shadow); also gates the render-block
    /// re-issue that reprojects the GPU-indirect terrain-detail pass (see
    /// `jc3vr_render::hooks::graphics_engine::scene::terrain`). Requires
    /// [`single_pass`](Self::enabled); independent so it can be A/B'd against the models.
    Terrain => terrain {
        default: true,
        rebuilds: true,
        label: "Single-pass the tessellated terrain (VS -> HS -> DS)",
        hover: "Rides the eye index through the terrain tessellation pipeline: the vertex shader \
                writes it on the free TEXCOORD3.z lane, the hull forwards it, and the domain \
                reprojects its clip by the per-eye M_eye. Covers the DrawIndexed terrain passes; \
                the GPU-indirect near passes stay double-drawn. Reloads shaders to apply.",
    }
    /// On top of [`terrain`](Self::terrain): reproject the terrain patch system's pipelines that
    /// cannot carry an eye index, inside the per-eye re-issue their draws already take. The patch
    /// system's tessellated domain shaders get no eye lane (their vertex shaders fill all of
    /// `TEXCOORD3`), and its GPU-indirect basemesh vertex shaders already read `SV_InstanceID`, so
    /// without this both are drawn once per eye from the centred camera. Each is rewritten to read
    /// its camera rows from `cb13`'s first eye slot (`dxbc_stereo::rebase_pinned_domain_shader` for
    /// the domain shaders, and `dxbc_stereo::remap_pinned_full_view_projection` for the basemesh),
    /// and each per-eye re-issue that finds one bound pins `cb13` to the eye it is drawing. No draw
    /// is added. The tessellation factors, the patch culling, and the distance fades stay
    /// centre-derived. A shader rewrite, so it needs a shader reload to apply. On by default.
    TerrainPatchPerEye => terrain_patch_per_eye {
        default: true,
        rebuilds: true,
        label: "Terrain patches per eye",
        hover: "Reprojects the terrain patch system's tessellated and GPU-indirect pipelines, \
                which cannot carry an eye index, by pinning cb13 to each eye in the per-eye \
                re-issue their draws already take. Off, both eyes of those draws come from the \
                centred camera. Adds no draws. Reloads shaders to apply.",
        requires: Terrain,
    }
    /// Draw the cloud layers from each eye under the double-wide collapse. The cloud vertex shaders
    /// (`clouds`, `cirrusclouds`, `cloudflythrough`, `softclouds`) take their clip position from the
    /// full view-projection, which the `cb0` remap does not make per eye, so both eye halves draw the
    /// clouds through the centred render camera, which carries eye 0's projection and no display
    /// cant: invisible with symmetric parallel eyes, but displaced in eye 1 under asymmetric frusta and
    /// in both eyes under canted ones. They take the `M_eye` reprojection instead. Needs a shader
    /// reload. On by default.
    CloudsPerEye => clouds_per_eye {
        default: true,
        rebuilds: true,
        label: "Clouds per eye",
        hover: "Reprojects the cloud vertex shaders by M_eye. Off, the clouds take their clip \
                position from the centred render camera in both eyes, which is wrong for \
                asymmetric or canted eyes. Reloads shaders to apply.",
    }
    /// Draw the bark and foliage blocks once for both eyes instead of re-issuing them per eye. The
    /// block's `Draw` runs once with each draw's vertex shader swapped for a reprojected variant
    /// (built when the engine creates the shader, while single-pass is enabled): a plain draw is
    /// promoted to two instances, a GPU-indirect draw is issued against a copy of its argument
    /// buffer with every instance count doubled (the variant halves the instance id it fetches by),
    /// and a hardware-instanced draw takes a step-rate-doubled layout twin when there is one and a
    /// per-eye re-issue of that one draw otherwise. A block whose vertex shader has no variant
    /// keeps the whole-block re-issue (see `jc3vr_render::stereo::single_pass::vegetation_once`).
    /// On by default; applies at once.
    VegetationOnce => vegetation_once {
        default: true,
        rebuilds: false,
        label: "Bark and foliage once for both eyes",
        hover: "Draws a bark or foliage block once instead of re-issuing it per eye: each draw \
                binds a reprojected variant of its vertex shader, plain draws take two instances, \
                GPU-indirect draws read a copy of their arguments with the instance counts \
                doubled, and hardware-instanced draws are re-issued per eye unless their layout \
                has a twin. Blocks without a variant keep the whole-block re-issue. Applies at \
                once.",
    }
    /// Re-issue an **already-instanced** draw once per eye under the collapse, instead of letting the
    /// patched vertex shader read the game's own instance ids as an eye parity. A `DrawIndexedInstanced`
    /// with a patched shader bound in the G-buffer range sends instance `i` to eye `i & 1` -- half the
    /// batch per eye, or the left eye alone at one instance. The instance count cannot simply be
    /// promoted (per-instance vertex-buffer stepping is indexed by the instance id, so doubling it reads
    /// past the instance data), so each eye gets its own submission with both `cb13` eye slots and both
    /// viewport slots pinned to that eye. Requires the collapse. On by default: it fixes a visible
    /// artifact (the buildings flickering), and costs ~130 extra draw submissions of ~20k per frame.
    InstancedPerEye => instanced_per_eye {
        default: true,
        rebuilds: false,
        label: "Already-instanced draws per eye (buildings, vegetation)",
        hover: "Re-issues a DrawIndexedInstanced whose patched shader is bound once per eye, with \
                both cb13 eye slots and both viewport slots pinned to that eye, so the game's own \
                instance ids stop being read as an eye parity. Off, the batch is split \
                alternately between the eyes -- the building flicker.",
    }
    /// Draw a hardware-instanced draw once for both eyes when its patched vertex shader is bound and
    /// its input layout has a step-rate-doubled twin: the twin is bound, the instance count doubled,
    /// and the viewport split, so each pair of instances reads one element and the instance parity
    /// picks the eye. The twins are built after injection by replaying `GeneralMkIII`'s declaration
    /// creation (see `jc3vr_render::stereo::single_pass::hw_instanced`), so this covers the buildings'
    /// `generalmkiiihwinstanced` family; a draw with any other layout keeps `instanced_per_eye`'s
    /// re-issue. On by default; applies at once.
    HwInstancedOnce => hw_instanced_once {
        default: true,
        rebuilds: false,
        label: "Hardware-instanced buildings once for both eyes",
        hover: "Draws a hardware-instanced building batch once, with its input layout swapped for \
                a twin whose per-instance step rates are doubled and its instance count doubled, \
                so each pair of instances shares one element and the instance parity picks the \
                eye. Draws whose layout has no twin keep the per-eye re-issue. Applies at once.",
    }
    /// Re-issue a **GPU-indirect** draw once per eye under the collapse, so it stops inheriting
    /// whatever viewport the previous draw left bound. `DrawIndexedInstancedIndirect` and
    /// `DrawInstancedIndirect` are how the near tessellating terrain patches
    /// (`CRenderBlockTerrainPatch`'s near passes) and the foliage block submit, and without the
    /// re-issue those draws rasterise eye 0's per-eye projection into the full double-wide
    /// viewport, a 2x horizontal stretch that reads as the geometry sliding across the screen at
    /// twice the camera's rate. The counts live in a GPU buffer and cannot be doubled, so each eye
    /// gets its own submission with both viewport slots pinned to that eye's half. Like the
    /// unpatched `DrawIndexed` re-issue, the geometry is then present and correctly sized in both
    /// eyes but has no parallax. Requires the collapse. On by default: it fixes a confirmed visible
    /// artifact.
    IndirectPerEye => indirect_per_eye {
        default: true,
        rebuilds: false,
        label: "GPU-indirect draws per eye (near terrain patches, foliage)",
        hover: "Re-issues a DrawIndexedInstancedIndirect / DrawInstancedIndirect once per eye \
                with both viewport slots pinned to that eye's half. Nothing detoured these entry \
                points, so the near tessellating terrain patches and the foliage inherited the \
                full double-wide viewport and were stretched 2x horizontally -- which reads as \
                them sliding across the screen at twice the camera's rate.",
    }
    /// Run the deferred clustered-lighting resolve once per eye under the collapse, each run masked to
    /// that eye's half of the double-wide target and reconstructing with that eye's own basis.
    ///
    /// The resolve is a fullscreen quad that rebuilds world positions from depth through the
    /// `ViewProjInv` the off-axis reconstruction
    /// (`jc3vr_render::hooks::graphics_engine::screen::reconstruction`) substitutes, and samples
    /// the sun-shadow cascade over them. Collapsed, that single quad covers both eye halves while
    /// the basis describes one eye's frustum: the left half gets that eye's frustum compressed 2x
    /// horizontally and the right half a basis unrelated to it. The error is a function of the view
    /// matrix, so it moves as the camera turns -- the shadows slide across the screen instead of
    /// staying on the world. Splitting the pass is the only fix; one draw cannot carry two bases.
    ///
    /// Each run is masked with a **scissor**, not a half viewport, so the quad keeps its
    /// one-to-one NDC-to-pixel mapping and its G-buffer sampling stays correct; the half's basis comes
    /// from folding the full-target-NDC to eye-NDC remap into the substituted inverse. The whole render
    /// block is re-issued, so its light-assignment phase runs twice (identically) as well. Requires the
    /// collapse. On by default: sliding shadows are a worse defect than the extra light-assignment
    /// pass is a cost, and the flag remains for an A/B in the headset.
    ReconstructPerEye => reconstruct_per_eye {
        default: true,
        rebuilds: false,
        label: "Deferred lighting resolve per eye (sun shadows)",
        hover: "Runs the deferred clustered-lighting resolve twice, each run scissor-masked to \
                one eye's half of the double-wide target and reconstructing depth with that eye's \
                own basis. Off, the one fullscreen quad covers both halves with one eye's basis, \
                and the error turns with the camera -- the sun shadows slide across the screen.",
    }
    /// Stage each eye's own camera position into the fragment `GlobalConstants` for the length of
    /// every masked per-eye run of a split fullscreen pass, with the matching correction to the
    /// sun-shadow cascade transform.
    ///
    /// The split hands each run that eye's reconstruction basis, but the engine stages the camera
    /// position once per pass from the centred render camera, so without this both halves measure
    /// their view vectors, fog distances, and shadow lookups from the centre. On by default.
    FullscreenCameraPerEye => fullscreen_camera_per_eye {
        default: true,
        rebuilds: false,
        label: "Fullscreen passes use each eye's camera position",
        hover: "Stages each eye's own camera position, and the matching sun-shadow lookup \
                correction, for every per-eye run of a split fullscreen pass. Off, both halves \
                measure view vectors, fog distances, and shadow lookups from the centred camera.",
    }
    /// Shade every pixel from the camera position of the eye whose half of the double-wide target it
    /// lies in.
    ///
    /// The engine stages the camera position once per pass from the centred render camera, and an
    /// instance-doubled draw covers both eyes in one submission, so without this every material that
    /// measures a view vector or a camera distance in its pixel shader (cloth sheen, Fresnel,
    /// specular, environment reflection, the sun-shadow lookup of the forward-lit families) is shaded
    /// from between the eyes. Every pixel shader that reads the camera position is rewritten to pick
    /// its eye's rows from a mod-owned buffer (`stereo::single_pass::eye_camera`). The rewrite needs a
    /// shader reload; turning this off makes the rewritten shaders read the centred rows at once. On
    /// by default.
    PixelCameraPerEye => pixel_camera_per_eye {
        default: true,
        rebuilds: true,
        label: "Geometry shades from each eye's camera position",
        hover: "Every pixel shader that measures a view vector or a camera distance reads the \
                camera position of the eye whose half its pixel lies in. Off, every \
                instance-doubled draw shades both eyes from the point between them: cloth sheen, \
                specular, Fresnel, and reflections differ from an eye rendered on its own. \
                Reloads shaders to apply the rewrite; turning it off takes effect at once.",
    }
    /// Measure the local point and spot lights from each eye's camera position, in the pixel shaders
    /// that [`pixel_camera_per_eye`](Self::pixel_camera_per_eye) rewrites.
    ///
    /// The engine stores every local light's position, and every projected spot light's projection,
    /// relative to the centred render camera. Once a pixel's position is measured from its eye, each
    /// light vector is off by the eye's offset from that camera, which moves the lights near the
    /// camera (a held device's glow) visibly between the eyes. The rewrite subtracts the eye's offset
    /// from each light position the deferred resolve and the forward-lit families read
    /// (`stereo::single_pass::eye_camera`). It is part of the same shader rewrite, so turning it on
    /// needs a shader reload; turning it off zeroes the offset at once. On by default.
    LocalLightsPerEye => local_lights_per_eye {
        default: true,
        rebuilds: true,
        label: "Local lights measured from each eye",
        hover: "In the same rewritten pixel shaders, measures every local point and spot light \
                from the eye whose half the pixel lies in. The engine stores the lights relative \
                to the centred camera, so off, the lights near the camera (a held device's glow) \
                sit where the point between the eyes sees them. Reloads shaders to apply the \
                rewrite; turning it off takes effect at once.",
    }
    /// Lay the screen-door pattern out from each eye's own origin.
    ///
    /// The alpha-tested and fading materials (the vegetation, the `general` and `prop` families,
    /// buildings, the vehicle paint's stipple) discard against a threshold hashed from the pixel
    /// position, and the volumetric fog dithers its march by the same hash. The hash does not
    /// repeat horizontally, so in the right half of the double-wide target, whose pixel positions
    /// are offset by the left eye's width, a different set of pixels survives each fade than in an
    /// eye rendered on its own. `dxbc_stereo::localize_screen_door` rewrites the hash to read the
    /// position relative to the eye boundary `stereo::single_pass::eye_camera` publishes. A shader
    /// rewrite, so changing it needs a shader reload. On by default.
    ScreenDoorPerEye => screen_door_per_eye {
        default: true,
        rebuilds: true,
        label: "Screen-door fades laid out per eye",
        hover: "The alpha-tested and fading materials (vegetation, props, buildings, the vehicle \
                paint's stipple) and the volumetric fog dither by a pattern hashed from the pixel \
                position. Laid out from each eye's own origin, every eye keeps the same pixels as \
                an eye rendered on its own. Off, the right eye's pattern is shifted by the left \
                eye's width, and a different set of pixels survives each fade. Reloads shaders to \
                apply.",
    }
    /// Read the render camera's position per eye in the remapped shaders that subtract it, rather
    /// than the pass camera's, before the translation-free view-projection.
    ///
    /// The road surfaces (`layeredroad`) and the `roadbias` model permutations take the camera
    /// position they subtract from the world position from the render camera's row of the vertex
    /// `GlobalConstants` (`cb0[9]`), not from the pass camera's row the remap gives each eye
    /// (`cb0[4]`). Remapped, they project each eye from the centred camera's position, so the
    /// surface slides half the eye distance within its own plane: the depth agrees while the
    /// texturing does not. `dxbc_stereo::ClipOrigin::PerEye` reads the row from each eye's copy in
    /// `cb13`. A shader rewrite, so changing it needs a shader reload. On by default.
    ClipOriginPerEye => clip_origin_per_eye {
        default: true,
        rebuilds: true,
        label: "Road clip origin per eye",
        hover: "The road surfaces and the road-biased models subtract the render camera's \
                position, not the pass camera's, before projecting. Read per eye, each eye \
                projects them from its own position. Off, both eyes project them from the centred \
                camera, so the road's texture slides half the eye distance across its surface. \
                Reloads shaders to apply.",
    }
    /// Cull the terrain's tessellated patches against a frustum that bounds both eyes under the
    /// collapse. The terrain's hull programs discard a patch outside the offset view-projection the
    /// block types bake once per frame per slot from the render context
    /// ([`RenderBlockTypeTerrain::SetupConstantBuffers`](jc3gi::graphics_engine::render_block::RenderBlockTypeTerrain::SetupConstantBuffers)
    /// and
    /// [`RenderBlockTypeTerrainPatch::SetupConstantBuffers`](jc3gi::graphics_engine::render_block::RenderBlockTypeTerrainPatch::SetupConstantBuffers)),
    /// which under the collapse is the centred camera's: eye 0's projection, with no cant. A canted
    /// eye sees past that frustum's outer edge, so the far terrain there was discarded for the left
    /// eye (the right eye's outer side lies inside the double-wide target's clip range). With this on,
    /// the bake inside the G-buffer range takes its horizontal and vertical rows from the projection
    /// bounding both eyes ([`crate::vr::eyes_union_projection_standard`]); the domain programs read
    /// each eye's own rows there, so only the cull sees the substitution. Applies without a shader
    /// reload. On by default.
    TerrainCullBothEyes => terrain_cull_both_eyes {
        default: true,
        rebuilds: false,
        label: "Terrain culled for both eyes",
        hover: "Culls the tessellated terrain patches against a frustum bounding both eyes. Off, \
                the hull programs cull them against the centred camera's frustum, which a canted \
                eye sees past at its outer edge: distant hills go missing there.",
    }
    /// Give the WaveWorks water blocks (`NvWater*`) a per-eye view under the collapse, so the water
    /// surface has parallax instead of being one eye's view shown to both.
    ///
    /// These blocks are not affected by the legacy water's projective-UV defect: their shaders derive
    /// the screen UV from `SV_Position` and the inverse screen size, which is already consistent with
    /// a double-wide target. Their defect is the other one — the vertex shader writes clip position
    /// from a baked model-view-projection in its own constant buffer rather than from the render
    /// context, so the collapse's per-eye machinery never reaches it and both eyes see the collapsed
    /// centre view. Flat water at the wrong depth reads as correct in a screenshot and wrong in a
    /// headset, which makes the defect easy to miss. On by default -- the flag stays so a suspected
    /// regression can be isolated without rebuilding.
    NvwaterPerEye => nvwater_per_eye {
        default: true,
        rebuilds: false,
        label: "WaveWorks water per eye (parallax)",
        hover: "Gives the NvWater* blocks a per-eye view. Their screen-space reads were always \
                fine; what is missing is parallax -- the vertex shader transforms from a baked \
                matrix in its own constant buffer, so both eyes get the collapsed centre view. \
                Flat water at the wrong depth looks right on the mirror and wrong in the headset.",
    }
    /// Give **screen-space decals** a per-eye block intercept under the collapse.
    ///
    /// `CRenderBlockSSDecal` is not a `PerspectiveFovInverse` consumer: its type-level `Setup` builds
    /// its own reconstruction basis inline and uploads it to fragment `cb1[0..3]`, and its pixel shader
    /// derives the depth-fetch UV *projectively* from its own clip position — normalized over one eye
    /// while the depth buffer is double-wide. So the decal reconstructs from the wrong surface, and the
    /// error moves with the camera.
    ///
    /// Both halves need fixing together: re-upload that eye's basis per eye, **and** bias the shader's
    /// projective UV into that eye's half of the buffer. Note the same `uv` feeds the reconstruction
    /// matrix (which wants the per-eye value) and the depth fetch (which wants the double-wide one), so
    /// they must stay separate. On by default.
    SsdecalPerEye => ssdecal_per_eye {
        default: true,
        rebuilds: true,
        label: "Screen-space decals per eye",
        hover: "Re-uploads each eye's reconstruction basis for the SSDecal block and biases its \
                projective depth-fetch UV into that eye's half of the double-wide buffer. Off, \
                the decal reconstructs its surface from the wrong part of the depth buffer and \
                the error moves with the camera. Reloads shaders to apply: the depth-fetch bias \
                is a shader rewrite, and until the reload the already rewritten permutations keep \
                reading it.",
    }
    /// Reproject the screen-space decal *box geometry* per eye, on top of
    /// [`ssdecal_per_eye`](Self::ssdecal_per_eye), which fixes only where the decal reconstructs
    /// from.
    ///
    /// The block bakes a world-view-projection into its vertex constants, so like the water blocks its
    /// geometry never sees the collapse's per-eye transform: the decal lands on the right surface in
    /// both eyes but its screen coverage has no parallax. Separate flag because the reconstruction fix
    /// is the one that stops the sliding, and this one only adds depth to it. On by default.
    SsdecalGeometryPerEye => ssdecal_geometry_per_eye {
        default: true,
        rebuilds: false,
        label: "...and reproject the decal box geometry",
        hover: "Sub-option of the decal fix: reprojects the decal box's baked transform per eye \
                so its screen coverage has parallax too. The reconstruction fix alone stops the \
                sliding; this adds depth to it.",
    }
    /// Lay the static **foveation** out over each eye's half of the double-wide target under the
    /// collapse.
    ///
    /// The foveation's mask and fill-in passes place one radial drop around one centre, sized from
    /// the target's half-diagonal. Under the collapse they run once, so the full-rate fovea sits on
    /// the seam with a radius sized for the double-wide target, and each eye drops pixels at its own
    /// centre of view and keeps them at its inner edge. With this on, both passes take both eyes'
    /// centres and an eye-sized radius, and each pixel uses its own eye's, with its dither and its
    /// fill-in neighbourhood in eye-local pixels, in the same single draw. Applies at once. On by
    /// default.
    FoveationPerEye => foveation_per_eye {
        default: true,
        rebuilds: false,
        label: "Foveation per eye",
        hover: "Centres the foveation's full-rate region and its peripheral drop on each eye, \
                sized for one eye. Off, one fovea sits on the seam, sized for the double-wide \
                frame, so each eye drops pixels at its own centre of view.",
    }
    /// Force the **foveation**'s peripheral stencil test through the G-buffer passes that write
    /// only their own pixels and leave the depth to the prepasses (the terrain's apply passes, the
    /// road junctions, and the decals), not just the models, the vegetation, and the creatures.
    ///
    /// Gates both modes, multipass included: it lives with the single-pass switches so the harness
    /// can compare it within one schedule. The kept pixels render as before; the dropped ones are
    /// shaded by fewer passes before the fill-in replaces them. Applies at once. On by default.
    FoveationGbuffer => foveation_gbuffer {
        default: true,
        rebuilds: false,
        label: "Foveation over the G-buffer",
        hover: "Skips the foveation's dropped pixels in the terrain's apply passes, the road \
                junctions, and the decals, in multipass too. Off, only the models, the \
                vegetation, and the creatures skip them.",
    }
    /// Force the **foveation**'s peripheral stencil test through the lighting passes that write only
    /// their own pixels (the deferred lights, the light billboards, the stars, sun, moon, skybox, and
    /// sky gradient, and the fog gradient), and run the fill-in right after them rather than at the
    /// start of the post effects.
    ///
    /// The passes after the fill-in (the water, the clouds, the fog volumes, and the transparents)
    /// then shade the reconstructed pixels as they shade every other, and read a reconstructed
    /// neighbourhood where they read the lit colour's. Gates both modes, as
    /// [`foveation_gbuffer`](Self::foveation_gbuffer) does. Applies at once. On by default.
    FoveationLighting => foveation_lighting {
        default: true,
        rebuilds: false,
        label: "Foveation over the lighting",
        hover: "Skips the foveation's dropped pixels in the deferred lights, the sky, and the fog \
                gradient, and fills them in right after, before the water and the transparents, \
                in multipass too. Off, the fill-in runs at the start of the post effects.",
    }
    /// Drop the **foveation**'s peripheral pixels in whole 2 by 2 quads rather than one by one.
    ///
    /// The rasterizer shades pixels in 2 by 2 quads, and a quad with one live pixel runs the pixel
    /// shader for all four; with single dropped pixels nearly every quad keeps one, so the stencil
    /// test skips little shading. Dropped quads have no live pixel and run nothing. The fill-in
    /// reaches two pixels to find kept neighbours. Gates both modes. Applies at once. On by default.
    FoveationQuads => foveation_quads {
        default: true,
        rebuilds: false,
        label: "Foveation drops whole quads",
        hover: "Drops the foveation's peripheral pixels in 2 by 2 quads, so a dropped quad runs \
                no pixel shader at all, in multipass too. Off, single pixels are dropped and \
                nearly every quad still shades.",
    }
    /// Run **FSR** once per eye over that eye's half of the double-wide target under the collapse.
    ///
    /// FSR otherwise runs once over the whole target with one history: the motion vectors, which
    /// the velocity pass writes in each eye's own screen space, are scaled to the double-wide width,
    /// so the history is reprojected twice as far sideways as the eye moved, and the history and the
    /// reconstruction's neighbourhoods reach across the seam into the other eye. With this on, each
    /// eye's colour, depth, and motion vectors are copied into eye-sized inputs and resolved through
    /// that eye's own context, as an eye rendered on its own is, with the stereo motion-vector
    /// correction taken from that eye's own matrices, and the result is copied back into the eye's
    /// half. Two FSR dispatches instead of one; no draws are added. Applies at once, with a history
    /// reset. On by default.
    FsrPerEye => fsr_per_eye {
        default: true,
        rebuilds: false,
        label: "FSR per eye",
        hover: "Resolves FSR once per eye over that eye's half, each with its own history, as an \
                eye rendered on its own is. Off, one FSR pass covers both eyes: its history moves \
                twice as far sideways as the eye did and reaches across the seam.",
    }
    /// Compute the **SSAO** pass for each eye under the collapse, in the block's single run.
    ///
    /// Collapsed, the occlusion generation reconstructs the whole double-wide target from one basis
    /// and one view rotation, its taps reach twice as far horizontally and read the other eye's depth
    /// near the seam, and the right eye's noise is offset by the eye's width. This rewrites the
    /// generation shader to compute each half for its own eye from a per-eye constant pair the block
    /// binds (`stereo::single_pass::ambient_occlusion`). A shader rewrite: changing it requests a
    /// shader reload. On by default.
    SsaoPerEye => ssao_per_eye {
        default: true,
        rebuilds: true,
        label: "SSAO per eye",
        hover: "Computes ambient occlusion for each eye's half in the pass's one run: each eye's \
                basis and view rotation, taps kept to the eye's half and scaled to it, and \
                eye-local noise. Reloads shaders to apply: it is a shader rewrite.",
    }
    /// Compute the **SSAO** passes' coordinates, texels, and filter weights in the shader, in every
    /// mode.
    ///
    /// The SSAO passes sample at places where the sampler's choice of texel or weight rests on the
    /// coordinate's last bits: the generation's centre samples sit exactly on a full-resolution texel
    /// boundary, the bilateral blur steps by half a texel, and the composite's bilinear upsample
    /// quantizes its weights. The rasterizer's interpolated UV and the sampler's conversion of a
    /// normalized coordinate both behave differently in an eye-sized target and in the double-wide
    /// one, so the two chose differently at some pixels, in stripes 14 columns apart across the eye
    /// that the blur spread and the composite spread again. With this on, every mode computes the UV
    /// from the pixel position over a size read from a texture, moves the generation's centre samples
    /// off the boundary, reads the blur's samples through loads of the texel the coordinate floors
    /// to, and blends the composite's four texels in the shader, so the left eye's occlusion is the
    /// isolated eye's wherever their inputs agree. Under the collapse the generation also places its
    /// taps in the eye's own UV and takes only the samples onto the eye's half. It changes the
    /// isolated eyes' and multipass's occlusion too, by which of two adjacent texels a sample reads
    /// and by a weight's last bits. A shader rewrite: changing it requests a shader reload. On by
    /// default.
    SsaoExactSampling => ssao_exact_sampling {
        default: true,
        rebuilds: true,
        label: "SSAO exact sampling",
        hover: "Computes the ambient occlusion's coordinates, texels, and blur and upsample \
                weights in the shader in every mode, so an eye rendered on its own and each eye \
                of the double-wide frame read the same texels with the same weights. Off, the \
                rasterizer and the sampler round differently at the two widths, which shows as \
                faint vertical stripes in the occlusion on the ground. Also changes multipass. \
                Reloads shaders to apply.",
    }
    /// Shade the ambient-occlusion volumes for both eyes from one draw.
    ///
    /// Each volume (`aobox`, `aosphere`) maps the scene under it into its own unit space through a
    /// matrix the block builds from the render context's camera, sampling the depth at the screen UV.
    /// Under the collapse that camera is the centred one and the depth spans both eyes, so each eye's
    /// half reads the depth of the wrong place (the left half of each eye reads the left eye, the
    /// right half the right) and maps it through the wrong camera. The volume's vertex shader takes
    /// the reprojection, so its draw is instance-doubled, and
    /// `dxbc_stereo::localize_ambient_occlusion_volume` rewrites its pixel shaders to sample at the
    /// pixel's double-wide UV and map each depth through the eye, from rows
    /// `stereo::single_pass::ao_volumes` binds for the pass. Shader rewrites, so changing it needs a
    /// shader reload. On by default.
    AoVolumesPerEye => ao_volumes_per_eye {
        default: true,
        rebuilds: true,
        label: "Ambient-occlusion volumes per eye",
        hover: "Draws the ambient-occlusion boxes and spheres once for both eyes: the volume is \
                projected into each eye, and each pixel samples its own eye's depth and maps it \
                through its own eye's camera. Off, each eye's half reads the depth of the wrong \
                place through the centred camera, which in tunnels and under cover shows as an \
                edge down the middle of each eye. Reloads shaders to apply.",
    }
    /// Trace the **screen-space reflections** for each eye under the collapse, in the block's single
    /// run.
    ///
    /// Collapsed, the ray march projects both halves of the double-wide target through one eye-sized
    /// projection onto the double-wide depth, rebuilds both from one view ray basis, and looks the
    /// previous frame up through the double-wide previous view-projection. This rewrites the ray
    /// march to trace each half for its own eye from per-eye rows the block binds, and the blur to
    /// keep its taps to the eye's half (`stereo::single_pass::screen_space_reflection`). A shader
    /// rewrite: changing it requests a shader reload. On by default.
    SsrPerEye => ssr_per_eye {
        default: true,
        rebuilds: true,
        label: "SSR per eye",
        hover: "Traces screen-space reflections for each eye's half in the pass's one run: each \
                eye's view ray, view, projection, and history lookup, the march kept to the eye's \
                half, and the blur's taps clamped to it. Reloads shaders to apply: it is a shader \
                rewrite.",
    }
    /// Derive the collapse's eye-half viewports from the render target the engine currently has
    /// bound, rather than always from the scene's double-wide viewport.
    ///
    /// Several passes redirect their draws to a **reduced-resolution off-screen target**: the shared
    /// quarter-resolution buffer (half per axis) that the low-resolution clouds, the low-resolution
    /// particles, and the volumetric spot-light cones all render into, and the downsampled depth
    /// buffer. The collapse's viewport split is otherwise target-blind — it re-derives the halves from
    /// the recorded scene viewport before every draw in the range — so a draw into a half-sized target
    /// receives a viewport twice that target's dimensions. Its content is magnified 2x about the
    /// target's origin and cropped, and since the error is a fixed scale it is a 2x motion gain too:
    /// clouds and smoke sweep past at twice the camera's rate.
    ///
    /// The composes need no matching change. They stretch the whole low-resolution texture over the
    /// whole double-wide target with a baked UV, so a texture holding a left-half/right-half image
    /// lands as a left-half/right-half screen image on its own — and they must **not** be re-issued
    /// per eye, since both blend rather than overwrite.
    ///
    /// On by default. Everywhere except those passes the two records hold the same viewport and this is
    /// a no-op, so turning it off isolates it cleanly.
    CollapseViewportFollowsTarget => collapse_viewport_follows_target {
        default: true,
        rebuilds: false,
        label: "Eye viewports follow the bound target (clouds, smoke, spotlight volumetrics)",
        hover: "Splits the viewport of whatever render target the engine currently has bound, \
                instead of always splitting the scene's double-wide one. The low-resolution \
                clouds, particles, and spot-light cones render into a shared quarter-resolution \
                buffer (half per axis); handing those draws a full-scene viewport magnifies them \
                2x about the target's origin and crops them, which is also a 2x motion gain -- \
                clouds and smoke sliding at twice the camera's rate. A no-op everywhere else, \
                since the two viewports agree outside those passes.",
    }
    /// Give **geometry** drawn through the non-indexed `Draw` entry point (D3D11 context vtable slot
    /// 13) the same per-eye re-issue the indexed path gives its draws.
    ///
    /// Slot 13 is overwhelmingly how the fullscreen passes submit their triangle, so the collapse
    /// resets the viewport to the whole double-wide target for it. But the four decal blocks, the road
    /// layers, and the skidmarks submit ordinary world geometry non-indexed, and pinning the full
    /// viewport rasterises them across both eye halves, stretched 2x horizontally about the target
    /// centre. A 2x horizontal stretch is also a 2x horizontal *motion* gain, so they sweep across the
    /// screen at twice the camera's rate -- decals sliding over the world.
    ///
    /// Unlike the indirect and indexed paths this cannot be decided from the draw alone, since a
    /// fullscreen triangle and a decal box arrive identically; it is decided by an allowlist of passes
    /// known to carry geometry, derived by enumerating every caller of the two engine wrappers that
    /// reach the slot and taking pass membership from the pass-creation sites. On by default. The
    /// list stays an allowlist rather than a heuristic because misclassifying a fullscreen pass as
    /// geometry is a visibly wrong frame, while missing a geometry pass is only the status quo -- so
    /// a pass is added when it is evidenced, never on suspicion.
    Slot13PerEye => slot13_per_eye {
        default: true,
        rebuilds: false,
        label: "Non-indexed geometry per eye (decals, roads, skidmarks)",
        hover: "Re-issues a non-indexed Draw once per eye, with both viewport and cb13 eye slots \
                pinned to that eye, for the passes known to submit geometry this way. Off, every \
                slot-13 draw gets the whole double-wide viewport -- right for a fullscreen \
                triangle, but it stretches decals and road layers 2x horizontally, which is a 2x \
                motion gain and reads as them sliding over the world.",
    }
    /// Requires [`reconstruct_per_eye`](Self::reconstruct_per_eye), and rides on the off-axis froxel
    /// tile-bounds override: build the clustered (froxel) light grid **per eye** as well, instead of
    /// building it once with eye 0's projection against the double-wide tile count.
    ///
    /// The grid is a lattice of fixed-size pixel tiles over the framebuffer; collapsed, it is twice
    /// as wide as the frustum whose projection fills it, so every local light lands in the wrong
    /// tiles and the forward-lit families that sample it (foliage, glass, particles, blended
    /// materials -- ~20 render-block types) are lit wrongly in both eyes. The resolve re-issue
    /// already runs the whole block once per eye; this makes each run's light assignment cover only
    /// that eye's half of the tile grid, with that eye's projection and tile bounds.
    ///
    /// The two halves compose because the assignment blend is commutative, the halves are disjoint,
    /// and the compaction phase is per-tile-local -- so long as the second run's whole-target clear
    /// is suppressed, which is what makes the grid end the frame valid for **both** eyes. That
    /// matters well outside the block: foliage samples the grid a frame early, in the G-buffer range.
    ///
    /// Declines itself (leaving the un-split behaviour) when the eye seam of the double-wide render
    /// width does not fall on a tile boundary.
    ///
    /// On by default: the un-split grid mislights every forward-lit family, and the flag remains so
    /// the two can be compared in the headset.
    ClusteredPerEye => clustered_per_eye {
        default: true,
        rebuilds: false,
        label: "Clustered light grid per eye (local lights on forward materials)",
        hover: "Builds the 64-pixel froxel light grid once per eye, each run masked to that eye's \
                half of the tile grid with that eye's projection and tile bounds, and the second \
                run's clear suppressed so the halves compose. Off, the grid is built with eye 0's \
                projection against the double-wide tile count, so local lights land in the wrong \
                tiles for both eyes -- on the ~20 forward-lit families that sample it (foliage, \
                glass, particles, blended materials) as well as the deferred resolve. Needs the \
                per-eye resolve and the off-axis tile-bounds fix on. Declines itself if the eye \
                seam does not fall on a tile boundary.",
    }
    /// Skip the per-draw viewport bind of a routed draw when the two viewport slots already hold
    /// what it wants: every bind of the slots is recorded, the engine's and the mod's, so a pass's
    /// patched draws bind the split once rather than once each. Applies at once. On by default.
    ViewportBindCache => viewport_bind_cache {
        default: true,
        rebuilds: false,
        label: "Skip viewport binds the slots already hold",
        hover: "Records every bind of the two viewport slots, the engine's and the mod's, and \
                skips a routed draw's bind when the slots already hold what it wants, so a pass's \
                patched draws bind the eye split once rather than once each.",
    }
}

impl SinglePassSwitch {
    pub const COUNT: usize = Self::ALL.len();

    /// The switch with this wire name, if there is one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|switch| switch.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use super::*;

    /// The manifest the Python capture protocol reads the switch set from, relative to this crate.
    const MANIFEST: &str = "../../tools/rendering/protocol/single_pass_switches.json";

    /// The manifest's text for the table as it stands: each switch's wire name, default, and
    /// whether changing it needs a shader reload, in table order.
    fn manifest() -> String {
        let mut text = String::from(
            "{\n  \"generated_from\": \"crates/jc3vr-view/src/stereo/config/single_pass.rs\",\n  \"switches\": [\n",
        );
        for (index, switch) in SinglePassSwitch::ALL.iter().enumerate() {
            let separator = if index + 1 < SinglePassSwitch::COUNT {
                ","
            } else {
                ""
            };
            text.push_str(&format!(
                "    {{\"name\": \"{}\", \"default\": {}, \"rebuilds_shaders\": {}}}{separator}\n",
                switch.name(),
                switch.default_value(),
                switch.rebuilds_shaders(),
            ));
        }
        text.push_str("  ]\n}\n");
        text
    }

    /// The committed manifest matches the table. Run the test with `JC3VR_WRITE_MANIFEST=1` to
    /// rewrite it after changing the table.
    #[test]
    fn the_python_manifest_matches_the_table() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MANIFEST);
        let expected = manifest();
        if std::env::var_os("JC3VR_WRITE_MANIFEST").is_some() {
            std::fs::write(&path, &expected)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            committed, expected,
            "the single-pass switch manifest is stale: rerun this test with JC3VR_WRITE_MANIFEST=1"
        );
    }

    #[test]
    fn names_round_trip_and_are_unique() {
        for &switch in SinglePassSwitch::ALL {
            assert_eq!(SinglePassSwitch::from_name(switch.name()), Some(switch));
        }
        assert_eq!(SinglePassSwitch::from_name("enabled"), None);
        assert_eq!(SinglePassSwitch::from_name("ssr_per_eye "), None);
    }

    #[test]
    fn each_switch_owns_its_field_and_its_bit() {
        let base = SinglePassConfig::new();
        for &switch in SinglePassSwitch::ALL {
            assert_eq!(
                switch.get(&base),
                switch.default_value(),
                "{}",
                switch.name()
            );
            let mut changed = base;
            switch.set(&mut changed, !switch.get(&base));
            for &other in SinglePassSwitch::ALL {
                assert_eq!(
                    other.get(&changed) != other.get(&base),
                    other == switch,
                    "{} changed {}",
                    switch.name(),
                    other.name()
                );
            }
            assert_eq!(
                changed.switch_bits() ^ base.switch_bits(),
                1 << switch as u64,
                "{}",
                switch.name()
            );
        }
    }
}
