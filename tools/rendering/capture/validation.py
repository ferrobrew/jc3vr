"""Validation of capture artifacts written by the game."""

import dataclasses
import math
import hashlib
import json
import struct
import zlib
from pathlib import Path

from tools.rendering.comparison.stages import DxgiFormat, Source, Stage, artifact_path
from tools.rendering.protocol import (
    Capture,
    CascadeScale,
    Mode,
    NativeCapture,
    PhaseCapture,
    ProtocolError,
)
from tools.rendering.session.errors import SessionError


def read_object(path):
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise SessionError(f"capture: invalid JSON artifact {path}: {error}") from error
    if not isinstance(value, dict):
        raise SessionError(f"capture: expected a JSON object in {path}")
    return value


def physical_water_mvp(metadata, eye):
    base, = (r for r in metadata['stages'] if r['stage'] == 'base_sources')
    draws = [d for d in base['velocity']['draws'] if (d.get('projection') or {}).get('eye') == eye]
    matrices = {tuple(d['projection']['generated']) for d in draws}
    if (base['frame'] != metadata['predicted_display_time_ns'] or base['velocity']['id'] != metadata['id'] or
            sorted(d['args'][0] for d in draws) != [16, 48, 272, 496, 752] or len(matrices) != 1):
        raise ProtocolError('lake DS MVP: ambiguous physical-eye matrix source')
    words, = matrices
    if len(words) != 16:
        raise ProtocolError('lake DS MVP: incomplete matrix source')
    return [words[4*r+c] for r in (0, 2, 1, 3) for c in range(4)]


def validate_capture(folder: Path, capture_id: str, result: NativeCapture | Capture, mode: Mode | None = None,
                     generation: int | None = None, motion_capture: PhaseCapture | None = None,
                     water_inputs: bool = False, sun_inputs: bool = False,
                     shader_identity: bool = False, resolve_inputs: bool = False,
                     sun_depth_clip=None, water_reflection=None, cascade_scale: CascadeScale | None = None, water_surface_inputs: bool = False, water_tess_draw: int | None = None, patch_draw_ordinal: int | None = None, second_patch_ordinal: int | None = None, base_eye_vp: bool | None = None, group_atlas_scale: bool | None = None, aa_color_edge: bool | None = None, patch63_hull: bool | None = None, owner_four_domain: bool | None = None, lake_color_mirror: bool | None = None, lake_depth_edge: bool | None = None, lake_ds_mvp: bool | None = None, card_atlas: bool | None = None, group_fade: bool | None = None, group_cards: bool = False, group_residual: bool = False, lake_inner_edge: bool | None = None, aa_weights: bool = False, aa_draws: bool = False, lake_owner_four: bool = False, lake_producer: bool = False, lake_gradients: bool = False, lake_waves: bool = False, lake_normals: bool = False, lake_displacement: bool = False, lake_samplers: bool = False, writer_domain_camera: bool | None = None, tess_writer: bool = False, tess_owners: bool = False, base_projection: bool = False, base_sources: bool = False, terrain_interval: bool = False, group_inputs: bool = False, group_sources: bool = False, resolve_shadow: bool = False, dof_inputs: bool = False, fog_generation: bool = False, fog_inputs: bool = False, late_sky_stages: bool = False, resolve_samples: bool = False, resolve_eye_camera: bool | None = None, ssr_bracket: bool = False, ssr_operands: bool = False, ssr_prepare: bool = False, ssr_history: bool = False, early_depth: bool = False, velocity_trace: bool = False, bark_color_trace: bool = False, terrain_sources: bool = False, residual_bark: bool = False, left_interval: bool = False, impostor_inputs: bool = False, interval_trace: bool = False, post_geometry: bool = False, dof_edge_local: bool | None = None, fog_local_uv: bool | None = None, terrain_camera: bool | None = None, extra_bark_dither: bool | None = None, left_atlas_scale: bool | None = None, impostor_scale: bool | None = None, color_dither: bool | None = None, bark_dither: bool | None = None, proxy_eye: bool | None = None, march_eye: bool | None = None, veto_eye: bool | None = None, grid_eye: bool | None = None, history_center: bool | None = None) -> NativeCapture | Capture:
    metadata = read_object(folder / f"{capture_id}.capture.json")
    if motion_capture is not None and mode is None:
        raise ProtocolError("visual protocol: motion capture requires a diagnostic mode")
    if mode is not None:
        if not isinstance(result, Capture):
            raise ProtocolError("visual protocol: expected a diagnostic capture")
        if Capture.from_wire(metadata) != result:
            raise ProtocolError("visual protocol: screenshot metadata disagrees with its acknowledgement")
        result.validate(capture_id, mode, generation, validate_png(folder / f"{capture_id}.capture.png"),
                        motion_capture=motion_capture)
        if metadata.get("sun_depth_clip") != sun_depth_clip:
            raise ProtocolError("visual protocol: screenshot sun depth clip request mismatch")
        if sun_depth_clip is not None and mode == Mode.SINGLE_PASS:
            treatments = [record for record in metadata.get("stages", [])
                          if isinstance(record, dict) and record.get("stage") == "sun_depth_clip_treatment"]
            if (len(treatments) != 1 or treatments[0].get("eye") != 1 or
                    treatments[0].get("error") is not None or
                    treatments[0].get("frame") != metadata.get("predicted_display_time_ns") or
                    (treatments[0].get("sun_input") or {}).get("capture_id") != capture_id):
                raise ProtocolError("visual protocol: right-eye sun depth clip treatment was not applied")
        if metadata.get("patch_draw_ordinal") != patch_draw_ordinal or metadata.get("second_patch_ordinal") != second_patch_ordinal:
            raise ProtocolError("visual protocol: patch draw ordinal mismatch")
        if metadata.get("water_reflection") != water_reflection:
            raise ProtocolError("visual protocol: screenshot water reflection request mismatch")
        if metadata.get('patch63_hull') is not patch63_hull:
            raise ProtocolError('patch 63 hull request mismatch')
        if metadata.get('owner_four_domain') is not owner_four_domain:
            raise ProtocolError('owner four domain: request mismatch')
        if metadata.get('lake_color_mirror') is not lake_color_mirror:
            raise ProtocolError('lake color mirror: request mismatch')
        if metadata.get('lake_depth_edge') is not lake_depth_edge:
            raise ProtocolError('lake depth edge: request mismatch')
        if metadata.get('lake_ds_mvp') is not lake_ds_mvp:
            raise ProtocolError('lake DS MVP: request mismatch')
        if metadata.get('aa_color_edge') is not aa_color_edge:
            raise ProtocolError('AA treatment request mismatch')
        if metadata.get('aa_weights') is not (True if aa_weights else None):
            raise ProtocolError('AA weights request mismatch')
        if aa_weights:
            records = [r for r in metadata['stages'] if r['stage'] == 'aa_weights']
            eyes = [0] if mode == Mode.SINGLE_PASS else [i for i, live in enumerate(mode.rendered_eyes) if live]
            if sorted(r['eye'] for r in records) != eyes:
                raise ProtocolError('AA weights: missing physical dispatch')
            for r in records:
                w = metadata['eye_width'] * (1 + (mode == Mode.SINGLE_PASS))
                h = metadata['eye_height']
                source = r['source'] or {}
                if (r['error'] or r['frame'] != metadata['predicted_display_time_ns'] or
                        (source.get('width'), source.get('height'), source.get('bytes_per_pixel')) != (w,h,4) or
                        source.get('format_id') not in (27,28) or not r.get('raw') or
                        artifact_path(folder / f'{capture_id}.capture.json', r['raw']).stat().st_size != w*h*4):
                    raise ProtocolError('AA weights: invalid stage or raw size')
        if metadata.get('aa_draws') is not (True if aa_draws else None):
            raise ProtocolError('AA draw request mismatch')
        if aa_draws:
            records = [r for r in metadata['stages'] if r['stage'] == 'post_aa']
            eyes = [0] if mode == Mode.SINGLE_PASS else [i for i, live in enumerate(mode.rendered_eyes) if live]
            if sorted(r['eye'] for r in records) != eyes:
                raise ProtocolError('AA draws: missing physical dispatch')
            for r in records:
                aa = r.get('aa') or {}
                if (r['error'] or aa.get('frame') != r['frame'] or r['frame'] != metadata['predicted_display_time_ns'] or
                        aa.get('eye') != r['eye'] or aa.get('id') != capture_id or aa.get('overflow') is not False or aa.get('color_edge') is not aa_color_edge or
                        aa.get('mode') not in (0,1,2,3,4) or not 1 <= len(aa.get('draws', [])) <= 12 or
                        any(not valid_mapped_identity(d['ps']) or not d['target'] or not d['resources'] or not d['vertices'] for d in aa['draws'])):
                    raise ProtocolError('AA draws: incomplete scoped graphics evidence')
        if metadata.get('lake_owner_four') is not (True if lake_owner_four else None):
            raise ProtocolError('lake owner four: request mismatch')
        if metadata.get('lake_producer') is not (True if lake_producer else None):
            raise ProtocolError('lake producer: request mismatch')
        if metadata.get('lake_gradients') is not (True if lake_gradients else None):
            raise ProtocolError('lake gradients: request mismatch')
        if metadata.get('lake_waves') is not (True if lake_waves else None):
            raise ProtocolError('lake waves: request mismatch')
        if metadata.get('lake_normals') is not (True if lake_normals else None):
            raise ProtocolError('lake normals: request mismatch')
        if metadata.get('lake_displacement') is not (True if lake_displacement else None):
            raise ProtocolError("lake displacement: request mismatch")
        if lake_inner_edge is not None:
            if mode != Mode.SINGLE_PASS or not lake_samplers or metadata.get('lake_inner_edge') is not lake_inner_edge:
                raise ProtocolError('lake edge: unexpected treatment confirmation')
        elif metadata.get('lake_inner_edge') is not None:
            raise ProtocolError('lake edge: unrequested treatment')
        if water_reflection is not None:
            validate_water_treatment(folder, capture_id, metadata, mode)
        if metadata.get("cascade_scale") != cascade_scale:
            raise ProtocolError("visual protocol: screenshot cascade_scale request mismatch")
        if late_sky_stages:
            validate_late_sky_stages(folder, capture_id, metadata, mode)
        if tess_writer:
            validate_tess_writer(metadata, capture_id, mode, writer_domain_camera)
        elif terrain_sources:
            records = [r for r in metadata["stages"] if r["stage"] == "terrain_sources"]
            if len(records) != int(mode.rendered_eyes[0]):
                raise ProtocolError("terrain sources: missing left-eye record")
            for r in records:
                t = r.get("velocity") or {}
                draws = t.get("draws") or []
                if (r["error"] or r["frame"] != metadata["predicted_display_time_ns"] or r["eye"] != 0 or
                        t.get("id") != capture_id or t.get("packed") is not (mode == Mode.SINGLE_PASS) or
                        t.get("points") != [[204, 150], [307, 166]] or
                        [(d.get("terrain") or {}).get("ordinal") for d in draws] != [2, 4] or
                        any(d["pass"] != 56 or d["kind"] != "instanced_indirect" or not d["dsv_is_main"] or
                            d["terrain"].get("camera") is not terrain_camera or not d["terrain"]["hs"] or not d["terrain"]["ds"] or len(d["before"]) != 2 or len(d["after"]) != 2
                            for d in draws)):
                    raise ProtocolError(f"terrain sources: invalid pass-56 trace: {r.get('error')}")
        treatments = [r for r in metadata.get("stages", []) if r["stage"] == "fog_local_uv"]
        if (len(treatments) != int(fog_local_uv is True) or any(r['frame'] != metadata['predicted_display_time_ns'] or r['eye'] != 0 or r['error'] for r in treatments)):
            raise ProtocolError("fog reconstruction UV: treatment not verified")
        edge = [r for r in metadata.get('stages', []) if r['stage'] == 'dof_edge_local']
        if len(edge) != int(dof_edge_local is True) or any(r['eye'] != 0 or r['error'] or r['frame'] != metadata['predicted_display_time_ns'] for r in edge):
            raise ProtocolError('DoF edge: treatment not verified')
        if dof_inputs:
            eyes = [0] if mode == Mode.SINGLE_PASS else [i for i, live in enumerate(mode.rendered_eyes) if live]
            size = (metadata['eye_width'] * (2 if mode == Mode.SINGLE_PASS else 1), metadata['eye_height'])
            for stage in (Stage.DOF_SCENE, Stage.DOF_COMPOSITE, Stage.DOF_RETURNED):
                records = [r for r in metadata['stages'] if r['stage'] == stage]
                if sorted(r['eye'] for r in records) != eyes:
                    raise ProtocolError(f'DoF: missing or duplicate {stage} draw')
                for r in records:
                    if r['error'] or r['frame'] != metadata['predicted_display_time_ns'] or not r['viewports']:
                        raise ProtocolError(f"DoF: invalid {stage}: {r['error']}")
                    hdr = stage == Stage.DOF_SCENE
                    source = Source.from_wire(r['source'])
                    path = artifact_path(folder / f'{capture_id}.capture.json', r['raw' if hdr else 'png'])
                    ext = 'r11g11b10' if hdr else 'png'
                    if ((source.width, source.height) != size or path.name != f"{capture_id}.{stage}.eye{r['eye']}.{ext}" or
                        (hdr and (source.format != DxgiFormat.R11G11B10_FLOAT or path.stat().st_size != 4*size[0]*size[1])) or
                        (not hdr and validate_png(path) != size)):
                        raise ProtocolError(f'DoF: invalid {stage} artifact')
        if fog_inputs:
            expected = [0] if mode == Mode.SINGLE_PASS else [i for i, active in enumerate(mode.rendered_eyes) if active]
            for stage in (["fog_input", "fog_gen_depth", "fog_generated"] if fog_generation else ["fog_input"]):
                records = [r for r in metadata["stages"] if r["stage"] == stage]
                if sorted(r["eye"] for r in records) != expected:
                    raise ProtocolError(f"fog inputs: unexpected number of {stage} draws")
                for r in records:
                    if r["error"] or r["frame"] != metadata["predicted_display_time_ns"] or not r.get("raw"):
                        raise ProtocolError(f"fog inputs: invalid observation: {r.get('error')}")
                    desc = r["source"]
                    path = artifact_path(folder/f"{capture_id}.capture.json", r["raw"])
                    if path.name != f"{capture_id}.{stage}.eye{r['eye']}.raw" or path.stat().st_size != desc['width']*desc['height']*desc['bytes_per_pixel']:
                        raise ProtocolError("fog inputs: invalid raw artifact")
        if bark_color_trace:
            color = [r for r in metadata["stages"] if r["stage"] == "bark_color_trace"]
            if len(color) != int(mode.rendered_eyes[1]):
                raise ProtocolError("bark color trace: missing right record")
            for r in color:
                t = r.get("velocity") or {}
                if residual_bark and t.get("points") != [[215,292],[250,355],[239,320],[218,299],[249,354],[263,358]]:
                    raise ProtocolError("residual bark: unexpected source points")
                if t.get("dither") != color_dither or sorted(d["args"][0] for d in t.get("draws", []) if d.get("local_dither")) != ([36660,36760,36860,37960,38060,38160] if extra_bark_dither else [36660, 36860, 37960] if color_dither else []):
                    raise ProtocolError("bark color trace: treatment mismatch")
                if (r["error"] or r["frame"] != metadata["predicted_display_time_ns"] or r["eye"] != 1 or t.get("id") != capture_id or not t.get("draws")):
                    raise ProtocolError(f"bark color trace: invalid observation: {r.get('error')}")
        if tess_owners:
            packed = mode == Mode.SINGLE_PASS
            validate_terrain_owners(folder, capture_id, metadata, [0] if packed else [e for e, live in enumerate(mode.rendered_eyes) if live], metadata["eye_width"]*(1+packed),metadata["eye_height"])
        if base_sources:
            records = [r for r in metadata["stages"] if r["stage"] == "base_sources"]
            packed = mode == Mode.SINGLE_PASS
            if sorted(r['eye'] for r in records) != ([0] if packed else [i for i, live in enumerate(mode.rendered_eyes) if live]):
                raise ProtocolError("basemesh sources: missing physical record")
            for r in records:
                t = r.get('velocity') or {}
                if (r['error'] or r['frame'] != metadata['predicted_display_time_ns'] or t.get('id') != capture_id
                        or t.get('packed') is not packed or not t.get('draws') or t.get('points') != [[236,103],[188,105],[145,80],[99,79],[28,46],[29,47],[30,48],[144,76]]):
                    raise ProtocolError(f"basemesh sources: invalid frame or points: {r.get('error')}")
                if any(d['pass'] != 57 or not d['dsv_is_main'] or d['local_dither'] or d.get('group_atlas_scale') or
                       len(d['before']) != 8*(1+packed) or len(d['after']) != len(d['before']) for d in t['draws']):
                    raise ProtocolError("basemesh sources: invalid pass, depth target, or physical extent")
                if base_projection:
                    selected = [(d['args'][0], d['projection']['eye']) for d in t['draws'] if d.get('projection')]
                    required = [(offset, e) for offset in (16,48,272,496,752) for e in (range(2) if packed else [r['eye']])]
                    if any(d["projection"].get("applied") is not base_eye_vp for d in t["draws"] if d.get("projection")):
                        raise ProtocolError("basemesh VP: mismatched clone treatment")
                    if sorted(selected) != sorted(required):
                        raise ProtocolError("base projection: missing selected physical submission")
        if interval_trace:
            traces = [r for r in metadata["stages"] if r["stage"] == "interval_trace"]
            eye = 0 if left_interval else 1
            expected = ([0] if mode == Mode.SINGLE_PASS else [i for i, live in enumerate(mode.rendered_eyes) if live]) if group_sources else ([eye] if mode.rendered_eyes[eye] else [])
            if sorted(r["eye"] for r in traces) != expected:
                raise ProtocolError("interval trace: missing physical eye record")
            for r in traces:
                t = r.get("velocity") or {}
                if (r["error"] or r["eye"] not in expected or r["frame"] != metadata["predicted_display_time_ns"] or
                        t.get("id") != capture_id or t.get("packed") is not (mode == Mode.SINGLE_PASS) or
                        t.get("points") != ([[145,80],[99,79],[268,65],[273,63],[289,65],[289,64],[97,303],[302,65]] if group_residual else [[132,321],[134,314],[130,323],[125,332],[137,337],[97,303],[138,329],[147,320]] if group_sources else [[155,232],[132,233],[154,230],[65,225],[140,231],[249,351]]) or
                        not t.get("draws") or any(d["pass"] not in range(71,75) or d["local_dither"] for d in t["draws"])):
                    raise ProtocolError(f"interval trace: invalid submissions: {r.get('error')}")
                if group_sources and any(d["pass"] != 73 or not d["dsv_is_main"] or d["block_run"] != "TreeImpostor" or len(d["before"]) != 8 * (1 + (mode == Mode.SINGLE_PASS)) or len(d["before"]) != len(d["after"]) for d in t["draws"]):
                    raise ProtocolError("group sources: incomplete physical draw trace")
                cards = [(d['args'][0], d['card_atlas']) for d in t['draws'] if d.get('card_atlas') is not None]
                if sorted(cards) != ([(4872, card_atlas), (5946, card_atlas)] if card_atlas is not None else []):
                    raise ProtocolError("card atlas: physical submission mismatch")
                fades = [(d['args'][0], d['group_fade']) for d in t['draws'] if d.get('group_fade') is not None]
                if sorted(fades) != ([(6534, group_fade), (7542, group_fade)] if group_fade is not None and mode == Mode.SINGLE_PASS else []):
                    raise ProtocolError("impostor fade: physical submission mismatch")
                if [d["args"][0] for d in t["draws"] if d.get("group_atlas_scale")] != ([1392] if group_atlas_scale else []):
                    raise ProtocolError("group atlas scale: submission mismatch")
                if sum(bool(d.get("left_atlas_scale")) for d in t["draws"]) != int(left_atlas_scale is True):
                    raise ProtocolError("left atlas scale: submission mismatch")
                if sum(bool(d.get("physical_atlas_scale")) for d in t["draws"]) != int(impostor_scale is True):
                    raise ProtocolError("impostor scale: submitted treatment count mismatch")
                if impostor_inputs:
                    selected = [d for d in t["draws"] if d.get("impostor")]
                    if sorted(d["args"][0] for d in selected) != ([144,1392,3300,4872,5946,6534,7542] if group_cards else [144,1392,3300] if group_residual else [1392, 3300] if group_inputs else [3300]):
                        raise ProtocolError("impostor inputs: missing selected draw")
                    for d in selected:
                        count, inputs = d["args"][0], d["impostor"]
                        if len(inputs["cards"]) != count // 6 or len(inputs["indices"]) != count:
                            raise ProtocolError("impostor inputs: incomplete card or index range")
                        if count in (144,1392,4872,5946,6534,7542) and (d["physical_atlas_scale"] or d["left_atlas_scale"] or
                                              len((inputs.get("raster") or {}).get("atlas", [])) != 2):
                            raise ProtocolError("group inputs: unexpected treatment or missing atlas bindings")
        if velocity_trace:
            traces = [r for r in metadata["stages"] if r["stage"] == "velocity_trace"]
            if len(traces) != int(mode.rendered_eyes[1]):
                raise ProtocolError("velocity trace: unexpected number of right-eye traces")
            for record in traces:
                trace = record.get("velocity") or {}
                if residual_bark and trace.get("points") != [[215,292],[250,355],[239,320],[218,299],[249,354],[263,358]]:
                    raise ProtocolError("residual bark: unexpected source points")
                if trace.get("dither") != bark_dither or sorted(d["args"][0] for d in trace.get("draws", []) if d.get("local_dither")) != ([36660,36760,36860,37960,38060,38160] if extra_bark_dither else [36660, 36860, 37960] if bark_dither else []):
                    raise ProtocolError("velocity trace: bark dither submission mismatch")
                if (record["error"] or record["frame"] != metadata["predicted_display_time_ns"] or
                        record["eye"] != 1 or trace.get("id") != capture_id or
                        trace.get("packed") is not (mode == Mode.SINGLE_PASS) or not trace.get("draws")):
                    raise ProtocolError(f"velocity trace: invalid submission record: {record.get('error')}")
        if early_depth:
            prefix = ["before_z_pass_depth", "after_z_pass_depth", "after_z_velocity_depth"]
            if interval_trace:
                prefix.extend(["after_fins_depth", "after_group_depth", "after_group2_depth"])
            if terrain_interval:
                prefix.extend(['after_base_near_depth', 'after_tess_far_depth', 'after_base_far_depth', 'after_tess_color_depth', 'after_base_color_depth', 'after_apply_near_detailed_depth', 'after_apply_near_depth'])
            if post_geometry:
                prefix.extend(["before_dynamic_models_depth", "after_dynamic_models_depth", "after_static_models_depth", "after_opaque_vegetation_depth", "after_forest_depth"])
            for stage in prefix:
                eyes = [0] if mode == Mode.SINGLE_PASS else [i for i, active in enumerate(mode.rendered_eyes) if active]
                records = [r for r in metadata["stages"] if r["stage"] == stage]
                if sorted(r["eye"] for r in records) != eyes:
                    raise ProtocolError(f"early depth: missing {stage} physical records")
                for r in records:
                    width = metadata["eye_width"] * (1 + (mode == Mode.SINGLE_PASS))
                    height = metadata["eye_height"]
                    path = artifact_path(folder/f"{capture_id}.capture.json", r["raw"])
                    src = r["source"]
                    if (r["error"] or r["frame"] != metadata["predicted_display_time_ns"] or
                            (src["width"], src["height"], src["format_id"], path.stat().st_size) != (width,height,19,width*height*8)):
                        raise ProtocolError(f"early depth: invalid {stage} eye {r['eye']}")
        if ssr_bracket:
            for eye, required in enumerate(mode.rendered_eyes):
                if not required:
                    continue
                expected = [("ssr_ray_color",26),("ssr_ray_alpha",41),("ssr_blur_color",26),("ssr_blur_alpha",41)]
                if ssr_prepare:
                    expected.extend([("ssr_prep_depth", 41), ("ssr_prep_normal", 28), ("ssr_prep_plane", 41), ("ssr_prep_scene", None)])
                if ssr_operands:
                    expected.extend((f"ssr_ray_{name}", 2) for name in ("input", "normal", "march", "mask"))
                    expected.extend([("ssr_history_uv", 2), ("ssr_history_sample", 2), ("ssr_history_source", 26)] if ssr_history else [("ssr_ray_segment", 2), ("ssr_ray_world", 2)])
                for stage, fmt in expected:
                    rs=[r for r in metadata["stages"] if r["stage"]==stage and r["eye"]==eye]
                    if len(rs)!=1 or rs[0].get("error") or rs[0]["frame"]!=metadata["predicted_display_time_ns"]:
                        raise ProtocolError(f"SSR bracket: invalid {stage} eye {eye}: {[r.get('error') for r in rs]}")
                    rec=rs[0]; w=metadata["eye_width"]//2*(2 if mode == Mode.SINGLE_PASS else 1); h=metadata["eye_height"]//2
                    path=artifact_path(folder/f"{capture_id}.capture.json",rec["raw"])
                    if stage == "ssr_prep_scene":
                        src = rec["source"]
                        if (src["width"], src["height"]) not in ((w,h),(2*w,2*h)) or src["format_id"] not in (19,20,39,40,41) or path.stat().st_size != src["width"]*src["height"]*src["bytes_per_pixel"]:
                            raise ProtocolError("SSR preparation: invalid scene depth")
                        continue
                    if (rec["source"]["width"],rec["source"]["height"],rec["source"]["format_id"],path.stat().st_size)!=(w,h,fmt,w*h*(16 if fmt == 2 else 4)):
                        raise ProtocolError("SSR bracket: invalid output")
        for treatment, name in ((march_eye, "march"), (veto_eye, "veto"), (grid_eye, "grid"), (history_center, "history")):
            if treatment is None:
                continue
            stages = (f"ssr_{name}_native", f"ssr_{name}_physical" if name in ("grid", "history") else f"ssr_{name}_packed")
            trials = [r for r in metadata["stages"] if r["stage"] in stages]
            if (mode != Mode.SINGLE_PASS or len(trials) != 2 or {r["eye"] for r in trials} != {0, 1} or
                    any(r["stage"] != stages[treatment] or r["frame"] != metadata["predicted_display_time_ns"] or r.get("error") for r in trials)):
                raise ProtocolError(f"SSR {name}: missing or incorrect per-eye treatment")
        if proxy_eye is not None:
            stage = "proxy_eye_applied" if proxy_eye else "proxy_native_observed"
            counts = [sum(r["stage"] == stage and r["eye"] == eye and r["frame"] == metadata["predicted_display_time_ns"] and not r.get("error") for r in metadata["stages"]) for eye in range(2)]
            if counts[0] != 12 or counts[1] != (12 if proxy_eye else 0):
                raise ProtocolError(f"proxy trial: expected all 12 qualified native proxies, got {counts}")
        if resolve_eye_camera is not None:
            if mode != Mode.SINGLE_PASS:
                raise ProtocolError("resolve camera requires SPS")
            for eye, obs in enumerate(metadata["resolve_inputs"]["eyes"]):
                observation = obs.get("observation") or {}
                trial = observation.get("camera") or {}
                pos = metadata["single_pass"]["eyes"][eye]["cb13_camera_position"][:3]
                center = metadata["single_pass"]["center_camera_position"][:3]
                xyz = struct.pack("<3f", *pos)
                cb = observation["constant_buffers"][0]
                raw = bytearray(artifact_path(folder/f"{capture_id}.capture.json", cb["content"]["raw"]).read_bytes())
                if trial.get("restored") is not True or trial.get("eye_camera") != list(struct.unpack("<3I",xyz)) or len(raw)!=1520 or raw[64:76] != struct.pack("<3f",*center) or raw[64:76]==xyz:
                    raise ProtocolError("resolve camera: source, original, or restoration differs")
                if resolve_eye_camera:
                    raw[64:76] = xyz
                if hashlib.sha256(raw).hexdigest()!=trial.get("bound_sha256"):
                    raise ProtocolError("resolve camera: replacement content differs")
        if resolve_samples:
            for eye, required in enumerate(mode.rendered_eyes):
                if not required:
                    continue
                stages = ("resolve_half_samples", "resolve_full_samples", "resolve_world", "resolve_view")
                if resolve_shadow:
                    stages += ("resolve_shadow_terms", "resolve_shadow_tap")
                for stage in stages:
                    records = [r for r in metadata.get("stages", ()) if r["stage"] == stage and r["eye"] == eye]
                    if len(records) != 1 or records[0].get("error") or records[0]["frame"] != metadata["predicted_display_time_ns"]:
                        raise ProtocolError(f"resolve samples: missing {stage} eye {eye}")
                    r = records[0]
                    w, h = metadata["eye_width"], metadata["eye_height"]
                    width = w * (2 if mode == Mode.SINGLE_PASS else 1)
                    path = artifact_path(folder / f"{capture_id}.capture.json", r["raw"])
                    if (r["source"]["width"], r["source"]["height"], r["source"]["format_id"], path.name, path.stat().st_size) != (width, h, 2, f"{capture_id}.{stage}.eye{eye}.raw", width*h*16):
                        raise ProtocolError("resolve samples: trace extent or artifact mismatch")
                    if mode == Mode.SINGLE_PASS and (r.get("scissor_enabled") is not True or r["scissors"] != [dict(left=w*eye, right=w*(eye+1), top=0, bottom=h)]*2):
                        raise ProtocolError("resolve samples: physical scissor mismatch")
        if resolve_inputs:
            validate_resolve_inputs(folder, capture_id, metadata, mode, shader_identity, cascade_scale)
        if water_inputs:
            validate_water_inputs(folder, capture_id, metadata, mode, shader_identity, water_tess_draw, water_surface_inputs)
            if lake_samplers:
                for r in metadata['stages']:
                    expected = {'water_input_refraction': 2, 'water_input_refraction_depth': 0}.get(r['stage'])
                    if expected is not None:
                        sampler = r['water_input'].get('sampler')
                        if (not isinstance(sampler, list) or len(sampler) != 5 or sampler[0] != expected or
                                not all(type(x) is int for x in sampler)):
                            raise ProtocolError('lake refraction: missing selected draw sampler state')
        if sun_inputs:
            validate_sun_inputs(folder, capture_id, metadata, mode)
        return result
    capture = NativeCapture(*(getattr(result, item.name) for item in dataclasses.fields(NativeCapture)))
    if NativeCapture.from_wire(metadata) != capture:
        raise ProtocolError("visual protocol: native capture metadata disagrees with its acknowledgement")
    if capture.id != capture_id or not capture.gameplay_ready or not capture.xr_session_running:
        raise SessionError("capture: screenshot is not a gameplay XR capture")
    if capture.fresh_eyes != (True, True):
        raise SessionError("capture: screenshot is not a fresh gameplay XR pair")
    if capture.source not in ("eye_textures", "collapsed_back_buffer"):
        raise SessionError("capture: screenshot has an unknown source")
    width, height = validate_png(folder / f"{capture_id}.capture.png")
    if (width, height) != (capture.width, capture.height) or width != capture.eye_width * 2 or height != capture.eye_height:
        raise SessionError("capture: PNG dimensions disagree with the eye metadata")
    return capture


def validate_late_sky_stages(folder: Path, capture_id: str, metadata: dict, mode: Mode) -> None:
    eyes = (0,) if mode == Mode.SINGLE_PASS else tuple(i for i, active in enumerate(mode.rendered_eyes) if active)
    stages = (Stage.AFTER_APPLY_CLOUDS, Stage.BEFORE_FOG_VOLUME_APPLY, Stage.AFTER_FOG_VOLUME_APPLY)
    expected = {(stage, eye) for stage in stages for eye in eyes}
    width = metadata['eye_width'] * (2 if mode == Mode.SINGLE_PASS else 1)
    source = Source(width, metadata['eye_height'], DxgiFormat.R11G11B10_FLOAT)
    seen = set()
    try:
        for r in metadata['stages']:
            if r['stage'] not in stages:
                continue
            key = (r['stage'], r['eye'])
            if (type(r['eye']) is not int or key not in expected or key in seen or r['error'] is not None
                    or r['frame'] != metadata['predicted_display_time_ns'] or Source.from_wire(r['source']) != source):
                raise ValueError('unexpected, invalid, or duplicate frame/eye record')
            raw = artifact_path(folder / f'{capture_id}.capture.json', r['raw'])
            if (raw.name != f'{capture_id}.{key[0]}.eye{key[1]}.r11g11b10' or
                    raw.stat().st_size != width * source.height * source.format.pixel_bytes):
                raise ValueError('missing or incompatible raw HDR artifact')
            seen.add(key)
        if seen != expected:
            raise ValueError(f'missing frame/eye records: {sorted(expected - seen)}')
    except (KeyError, TypeError, ValueError, OSError) as error:
        raise ProtocolError(f'visual protocol: late sky stages: {error}') from error


def validate_tess_writer(metadata, capture_id, mode, writer_domain_camera):
    packed = mode == Mode.SINGLE_PASS
    eyes = [i for i, live in enumerate(mode.rendered_eyes) if live]
    records = [r for r in metadata['stages'] if r['stage'] == 'terrain_sources']
    if sorted(r['eye'] for r in records) != ([0] if packed else eyes):
        raise ProtocolError('terrain writer: missing physical record')
    extra = metadata.get('lake_owner_four') is True
    points = [[9,258],[18,354],[3,308],[0,282],[0,256],[23,306]] + ([[186,161],[187,162]] if extra else [])
    for r in records:
        t = r.get('velocity') or {}
        draws = t.get('draws') or []
        if (r['error'] or r['frame'] != metadata['predicted_display_time_ns'] or t.get('id') != capture_id or
                t.get('packed') is not packed or t.get('points') != points or
                len(draws) != (2 if packed else 1)*(2 if extra else 1)):
            raise ProtocolError(f"terrain writer: invalid pass-56 trace: {r['error']}")
        schedule = [(ordinal,eye) for ordinal in ([4,31] if extra else [31]) for eye in (eyes if packed else [r['eye']])]
        for (ordinal,eye), d in zip(schedule, draws, strict=True):
            s = d['terrain']; words = s.get('indirect') or []
            if (s['ordinal'] != ordinal or len(words) != 4 or not words[0] or not words[1] or
                    s['domain_camera'] is not (writer_domain_camera if ordinal == 31 else metadata.get('owner_four_domain') if eye == 1 else None) or s['camera'] is not None or s['gs'] or not all([d['vs'],s['hs'],s['ds'],(d['ps'] or {}).get('identity')]) or
                    d['pass'] != 56 or d['kind'] != 'instanced_indirect' or d['block_run'] != 'VolumetricTerrainPatch' or
                    d['args'] != [2016 if ordinal == 31 else 480,0,0,0,0] or d['depth_state'] != [1,1,7] or not d['dsv_is_main'] or
                    d['viewports'] != [[320*eye if packed else 0,0,320,360,0,1]]*(1+packed) or
                    len(d['before']) != len(points) or len(d['after']) != len(points)):
                raise ProtocolError('terrain writer: unexpected submission or viewport')
            albedo = s.get('albedo')
            if ordinal == 4:
                if not isinstance(albedo,list) or len(albedo)!=2 or any(len(v)!=2 or any(type(w) is not int or not 0<=w<2**32 for w in v) for v in albedo):
                    raise ProtocolError('terrain owner four: incomplete bracketed albedo')
            elif albedo is not None:
                raise ProtocolError('terrain owner four: unexpected albedo capture')
            buffers = {(c['stage'], c['slot']): c for c in d['cb']}
            required = {('vs',1):2, ('vs',2):6, ('vs',3):1, ('ds',0):2, ('ds',1):10, ('hs',0):1, ('hs',1):10}
            if packed:
                required['ds',13] = 18
            for key, rows in required.items():
                c = buffers.get(key) or {}
                if not c.get('range_known') or c.get('first') != 0 or len(c.get('bits') or []) != rows*4:
                    raise ProtocolError(f'terrain writer: incomplete {key} input')
            hull = metadata.get('patch63_hull') if packed and ordinal == 31 and eye == 1 else None
            if s.get('hull_camera') is not hull:
                raise ProtocolError('terrain hull: wrong draw or condition')
            if hull is not None:
                source = buffers['ds',13]['bits'][36:39]
                hs = buffers['hs',1]['bits']
                ds = buffers['ds',1]['bits']
                if ds[20:23] != source or hs[24:27] != (source if hull else ds[24:27]) or source == ds[24:27]:
                    raise ProtocolError('terrain hull: camera words differ from requested source')
            if ordinal == 4 and packed and eye == 1 and metadata.get('owner_four_domain') is not None:
                source = buffers['ds',13]['bits'][36:39] if metadata['owner_four_domain'] else buffers['hs',1]['bits'][24:27]
                if buffers['ds',1]['bits'][20:23] != source:
                    raise ProtocolError('owner four domain: bound camera differs from requested source')
            if buffers['vs',3]['bits'] != [63 if ordinal == 31 else 15,2,0,0]:
                raise ProtocolError('terrain writer: native vertex patch selector mismatch')


def validate_terrain_owners(folder,capture_id,metadata,eyes,width,height):
    records = [r for r in metadata['stages'] if r['stage'] == 'patch_depth_owners']
    if sorted(r['eye'] for r in records) != eyes:
        raise ProtocolError("visual protocol: missing terrain depth ownership map")
    for r in records:
        src = dict(width=width, height=height, bytes_per_pixel=4, format_id=42, format="DXGI_FORMAT(42)")
        if r['error'] or r['source'] != src or r['frame'] != metadata['predicted_display_time_ns']:
            raise ProtocolError(f"visual protocol: invalid terrain depth ownership: {r['error']}")
        path = artifact_path(folder / f"{capture_id}.capture.json", r['raw'])
        if path.name != f"{capture_id}.patch_depth_owners.eye{r['eye']}.raw" or path.stat().st_size != width * height * 4:
            raise ProtocolError("visual protocol: invalid terrain depth ownership file")


def validate_near_patch(folder, capture_id, metadata, mode):
    width = metadata["eye_width"] * (2 if mode == Mode.SINGLE_PASS else 1)
    height = metadata["eye_height"]
    eyes = [0] if mode == Mode.SINGLE_PASS else [e for e, live in enumerate(mode.rendered_eyes) if live]
    variant = metadata['water_reflection']
    camera = variant in ('lake_camera_control', 'lake_domain_camera', 'lake_hull_camera')
    brackets = ('tess_patch', 'patch_draw') if camera else ('tess_patch' if variant in ('lake_patch_draw', 'lake_patch_owners') else variant.removeprefix('lake_'),)
    if camera:
        verified = [r for r in metadata['stages'] if r['stage'] == 'domain_camera_submission']
        if (sorted(r['eye'] for r in verified) != ([0, 1] if mode == Mode.SINGLE_PASS else []) or
                any(r['error'] or r['frame'] != metadata['predicted_display_time_ns'] for r in verified) or
                (variant in ('lake_domain_camera','lake_hull_camera') and mode != Mode.SINGLE_PASS)):
            raise ProtocolError("visual protocol: missing or incompatible domain camera verification")
    if metadata.get('second_patch_ordinal') is not None:
        secondary = [r for r in metadata['stages'] if r['stage'] == 'second_camera_submission']
        if sorted(r['eye'] for r in secondary) != ([0, 1] if mode == Mode.SINGLE_PASS else []) or any(r['error'] or r['frame'] != metadata['predicted_display_time_ns'] for r in secondary):
            raise ProtocolError("terrain diagnostic: missing secondary camera submission")
    if variant == 'lake_patch_owners':
        validate_terrain_owners(folder,capture_id,metadata,eyes,width,height)
    for bracket in brackets:
        for boundary in ("before", "after"):
            for plane, fmt, stride in (("depth", 19, 8), ("albedo", 27, 4)):
                name = f"{boundary}_{bracket}_{plane}"
                records = [r for r in metadata["stages"] if r.get("stage") == name]
                if sorted(r["eye"] for r in records) != eyes:
                    raise ProtocolError(f"visual protocol: incorrect {name} eye records")
                for r in records:
                    source = dict(width=width, height=height, bytes_per_pixel=stride, format_id=fmt, format=f"DXGI_FORMAT({fmt})")
                    if r.get("frame") != metadata["predicted_display_time_ns"] or r.get("error") or r.get("source") != source:
                        raise ProtocolError(f"visual protocol: invalid {name} stage")
                    path = artifact_path(folder / f"{capture_id}.capture.json", r["raw"])
                    if path.name != f"{capture_id}.{name}.eye{r['eye']}.raw" or path.stat().st_size != width * height * stride:
                        raise ProtocolError(f"visual protocol: invalid {name} artifact")


def validate_water_treatment(folder: Path, capture_id: str, metadata: dict, mode: Mode) -> None:
    variant = metadata.get("water_reflection")
    if variant in ("lake_near_patch", "lake_tess_patch", "lake_patch_draw", "lake_patch_owners", "lake_camera_control", "lake_domain_camera", "lake_hull_camera"):
        validate_near_patch(folder, capture_id, metadata, mode)
        variant = "lake_response"
    uv = variant in ("planar_uv", "lake_samples", "lake_lighting", "lake_sky", "lake_sky_buffer", "lake_response")
    shader_condition = variant in ("lake_samples", "lake_lighting", "lake_sky", "lake_sky_buffer", "lake_response", "planar_uv", "pristine_lake", "center_planar", "packed_refraction", "projected_planar", "mirrored_planar", "eye_width_planar")
    zero_planar = metadata.get("water_reflection") == "zero_planar"
    allowed = (Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS) if zero_planar or uv else (Mode.SINGLE_PASS if shader_condition else Mode.MULTIPASS,)
    if mode not in allowed or not isinstance(metadata.get("stages"), list):
        raise ProtocolError("visual protocol: water reflection treatment requires multipass stages")
    records = metadata["stages"]
    eyes = [eye for eye, required in enumerate(mode.rendered_eyes) if required]
    if len([r for r in records if isinstance(r, dict) and r.get("stage") == "water_reflection_treatment"]) != len(eyes):
        raise ProtocolError("visual protocol: unexpected water treatment count")
    for eye in eyes:
        treatment = [r for r in records if isinstance(r, dict) and
                     r.get("stage") == "water_reflection_treatment" and r.get("eye") == eye]
        if len(treatment) != 1:
            raise ProtocolError("visual protocol: missing or duplicate water treatment")
        record = treatment[0]
        info = record.get("water_input")
        if (record.get("error") is not None or record.get("frame") != metadata.get("predicted_display_time_ns") or
                not isinstance(info, dict) or info.get("capture_id") != capture_id or
                type(info.get("treatment_draws")) is not int or info["treatment_draws"] <= 0):
            raise ProtocolError("visual protocol: water reflection treatment was not applied")
        if shader_condition:
            if info["treatment_draws"] != 1:
                raise ProtocolError("visual protocol: expected one planar treatment per eye")
            captures = [r for r in records if r.get("stage") == "water_input_static_reflection" and r.get("eye") == eye]
            identity = (captures[0].get("water_input") or {}).get("shader_identity") if len(captures) == 1 else None
            expected = {"lake_response": LAKE_RESPONSE_SHADERS[eye if mode == Mode.SINGLE_PASS else 2], "lake_sky_buffer": LAKE_LIGHTING_SHADERS[eye if mode == Mode.SINGLE_PASS else 2], "lake_sky": LAKE_LIGHTING_SHADERS[eye if mode == Mode.SINGLE_PASS else 2], "lake_lighting": LAKE_LIGHTING_SHADERS[eye if mode == Mode.SINGLE_PASS else 2], "lake_samples": LAKE_SAMPLE_SHADERS[eye if mode == Mode.SINGLE_PASS else 2], "planar_uv": PLANAR_UV_SHADERS[eye if mode == Mode.SINGLE_PASS else 2], "pristine_lake": LAKE_SHADER, "packed_refraction": PACKED_LAKE_SHADER,
                        "center_planar": CENTER_PLANAR_SHADERS[eye], "projected_planar": PROJECTED_PLANAR_SHADERS[eye],
                        "mirrored_planar": MIRRORED_PLANAR_SHADERS[eye], "eye_width_planar": EYE_WIDTH_SHADERS[eye]}[variant]
            if variant == 'lake_response' and eye == 0 and metadata.get('lake_inner_edge') is True:
                expected = LAKE_INNER_EDGE_SHADER
            if metadata.get("lake_displacement"):
                expected = displacement_shader(mode, eye, metadata)
            require_lake_shader(identity, expected)
            if variant in ("lake_samples", "lake_lighting", "lake_sky", "lake_sky_buffer", "lake_response"):
                for stage in GRADIENT_STAGES if metadata.get("lake_gradients") else WAVE_STAGES if metadata.get("lake_waves") else ("lake_sample_uv", "lake_sample_depth", "lake_sample_color", "lake_sample_planar") if variant == "lake_samples" else (tuple("lake_response_" + n for n in ("color", "uv", "tint", "output", "air")) + (("lake_normal_mesh", "lake_normal_mixed") if metadata.get("lake_normals") else ("lake_displacement_depth", "lake_displacement_surface") if metadata.get("lake_displacement") else ())) if variant == "lake_response" else ("lake_lighting_base", "lake_lighting_mixed", "lake_lighting_lit", "lake_lighting_output"):
                    recs = [r for r in records if r.get("stage") == stage and r.get("eye") == eye]
                    width = metadata["eye_width"] * (2 if mode == Mode.SINGLE_PASS else 1)
                    height = metadata["eye_height"]
                    if (len(recs) != 1 or recs[0].get("frame") != record["frame"] or recs[0].get("error") is not None
                            or recs[0].get("source") != dict(width=width, height=height, bytes_per_pixel=16, format_id=2, format="DXGI_FORMAT(2)")):
                        raise ProtocolError(f"visual protocol: invalid {stage} output for eye {eye}")
                    path = artifact_path(folder / f"{capture_id}.capture.json", recs[0]["raw"])
                    if path.name != f"{capture_id}.{stage}.eye{eye}.raw" or path.stat().st_size != width * height * 16:
                        raise ProtocolError(f"visual protocol: invalid {stage} raw artifact for eye {eye}")
            continue
        target = "water_input_static_reflection" if zero_planar else "water_input_reflection_alpha"
        inputs = [r for r in records if isinstance(r, dict) and r.get("stage") == target and r.get("eye") == eye]
        if len(inputs) != 1 or not isinstance(inputs[0].get("source"), dict):
            raise ProtocolError("visual protocol: missing treated reflection weight input")
        source = inputs[0]["source"]
        if zero_planar:
            binding = inputs[0].get("water_input") or {}
            identity = binding.get("shader_identity")
            if (info["treatment_draws"] != 1 or binding.get("shader_slot") != 2 or not valid_mapped_identity(identity) or
                    any(identity["identity"][k] != "99dd7036aa192c346c1e23883da83cf4da300dd1190dee40eedb3c2421531531" for k in ("actual_sha256", "pristine_sha256"))):
                raise ProtocolError("visual protocol: planar input substitution not observed on one lake draw")
        if (source.get("width"), source.get("height"), source.get("bytes_per_pixel"), source.get("format_id")) != (1, 1, 4, 26 if zero_planar else 41):
            raise ProtocolError("visual protocol: treated reflection weight has unexpected dimensions or format")
        try:
            raw = inputs[0].get("raw")
            if not isinstance(raw, str) or artifact_path(folder / f"{capture_id}.capture.json", raw).read_bytes() != bytes(4):
                raise ProtocolError("visual protocol: treated reflection weight is not zero")
        except (OSError, ValueError) as error:
            raise ProtocolError(f"visual protocol: invalid treated reflection artifact: {error}") from error


def validate_resolve_inputs(folder: Path, capture_id: str, metadata: dict, mode: Mode, shader_identity: bool,
                            cascade_scale: CascadeScale | None = None) -> None:
    capture = metadata.get("resolve_inputs")
    if (not isinstance(capture, dict) or type(capture.get("frame")) is not int or
            capture["frame"] != metadata.get("predicted_display_time_ns") or
            type(capture.get("generation")) is not int or not isinstance(metadata.get("visual"), dict) or
            capture["generation"] != metadata["visual"].get("generation") or
            not isinstance(capture.get("eyes"), list) or len(capture["eyes"]) != 2):
        raise ProtocolError("visual protocol: invalid resolve capture identity")
    for eye_index, (eye, required) in enumerate(zip(capture["eyes"], mode.rendered_eyes)):
        if not isinstance(eye, dict) or eye.get("status") != ("observed" if required else "not_required"):
            raise ProtocolError("visual protocol: requested resolve eye is missing or ambiguous")
        if not required:
            continue
        observation = eye.get("observation")
        if (not isinstance(observation, dict) or not isinstance(observation.get("pixel_shader"), dict) or
                type(observation["pixel_shader"].get("pointer")) is not int or observation["pixel_shader"]["pointer"] <= 0):
            raise ProtocolError("visual protocol: invalid resolve shader observation")
        counters = observation.get("frame_counters")
        if not isinstance(counters, dict) or any(type(counters.get(key)) is not int or not 0 <= counters[key] <= 0xffffffff
                                                 for key in ("counter", "frame_index", "ring_index")):
            raise ProtocolError("visual protocol: invalid resolve frame counters")
        buffers = observation.get("constant_buffers")
        if not isinstance(buffers, list) or len(buffers) != 4:
            raise ProtocolError("visual protocol: invalid resolve constant buffer list")
        for slot, buffer in enumerate(buffers):
            if (not isinstance(buffer, dict) or type(buffer.get("slot")) is not int or buffer["slot"] != slot or
                    any(type(buffer.get(key)) is not int or buffer[key] < 0 for key in ("pointer", "byte_width"))):
                raise ProtocolError("visual protocol: invalid resolve constant buffer")
            first, count = buffer.get("first_constant"), buffer.get("num_constants")
            if (first is None) != (count is None) or (first is not None and
                    (type(first) is not int or type(count) is not int or first < 0 or count < 0 or
                     first > 4096 or count > 4096)):
                raise ProtocolError("visual protocol: invalid resolve constant buffer range")
            if (buffer["pointer"] == 0) != (buffer["byte_width"] == 0):
                raise ProtocolError("visual protocol: inconsistent resolve constant buffer binding")
            if slot in (0, 3):
                content = buffer.get("content")
                if not isinstance(content, dict) or not isinstance(content.get("status"), str):
                    raise ProtocolError("visual protocol: missing resolve constant buffer content status")
                status = content["status"]
                if status == "captured":
                    raw = content.get("raw")
                    if not isinstance(raw, str) or first is None or count is None or count == 0 or count * 16 > 65536 or first * 16 + count * 16 > buffer["byte_width"]:
                        raise ProtocolError("visual protocol: invalid resolve constant buffer capture")
                    try:
                        path = artifact_path(folder / f"{capture_id}.capture.json", raw)
                        if path.name != f"{capture_id}.resolve_cb{slot}.eye{eye_index}.raw" or path.stat().st_size != count * 16:
                            raise ProtocolError("visual protocol: resolve constant buffer artifact mismatch")
                    except (OSError, ValueError) as error:
                        raise ProtocolError(f"visual protocol: invalid resolve constant buffer artifact: {error}") from error
                elif status not in ("unbound", "unknown_range", "empty_range", "oversize", "invalid_range", "read_error"):
                    raise ProtocolError("visual protocol: unknown resolve constant buffer content status")
                elif status == "read_error" and not isinstance(content.get("error"), str):
                    raise ProtocolError("visual protocol: invalid resolve constant buffer error")
            elif "content" in buffer:
                raise ProtocolError("visual protocol: unexpected resolve constant buffer content")
        resources = observation.get("resources")
        slots = (*range(15), *range(16, 32), 44, 45)
        if not isinstance(resources, list) or len(resources) != len(slots):
            raise ProtocolError("visual protocol: invalid resolve resource list")
        for slot, resource in zip(slots, resources):
            if (not isinstance(resource, dict) or type(resource.get("slot")) is not int or resource["slot"] != slot or
                    any(type(resource.get(key)) is not int or resource[key] < 0 for key in
                        ("view", "resource", "format", "dimension"))):
                raise ProtocolError("visual protocol: invalid resolve resource binding")
            texture = resource.get("texture")
            if resource["view"] == 0 and (resource["resource"] != 0 or texture is not None):
                raise ProtocolError("visual protocol: inconsistent resolve resource binding")
            if texture is not None and (not isinstance(texture, dict) or any(
                    type(texture.get(key)) is not int or texture[key] <= 0
                    for key in ("width", "height", "depth", "mip_levels", "array_size", "format"))):
                raise ProtocolError("visual protocol: invalid resolve texture description")
        identity = observation["pixel_shader"].get("identity")
        if shader_identity and not valid_mapped_identity(identity):
            raise ProtocolError("visual protocol: resolve pixel shader identity is unmapped")

        trial = observation.get("cascade")
        if cascade_scale is None:
            if trial is not None:
                raise ProtocolError("visual protocol: unexpected cascade treatment")
            continue
        cb = buffers[0]
        if (mode != Mode.MULTIPASS or not valid_mapped_identity(identity) or
                identity["identity"]["pristine_sha256"] != "06f43db0887b0038147e273a3879a9b7f48c3b630955a94d695898fd7d0e52fc" or
                identity["identity"]["actual_sha256"] != "6a82533e6458166ea3d04789c266ec127baf59931882955c99c4603e47f11c9b" or
                (cb["first_constant"], cb["num_constants"], cb["byte_width"]) != (0, 95, 1520) or
                cb["content"]["status"] != "captured" or not isinstance(trial, dict) or
                trial.get("applied") != cascade_scale.bits or trial.get("restored") is not True):
            raise ProtocolError(f"visual protocol: invalid cascade treatment for eye {eye_index}")
        raw = bytearray(artifact_path(folder / f"{capture_id}.capture.json", cb["content"]["raw"]).read_bytes())
        if trial.get("original") != struct.unpack_from("<I", raw, 54 * 16)[0]:
            raise ProtocolError("visual protocol: original cascade bits mismatch")
        struct.pack_into("<I", raw, 54 * 16, cascade_scale.bits)
        if hashlib.sha256(raw).hexdigest() != trial.get("bound_sha256"):
            raise ProtocolError("visual protocol: bound cascade content mismatch")


LAKE_SHADER = "99dd7036aa192c346c1e23883da83cf4da300dd1190dee40eedb3c2421531531"
PACKED_LAKE_SHADER = "2de4e3c8c3174fd998bc3d26498fc5698fcacc1aa67a5f92c40b87a28748f908"


GRADIENT_STAGES = ('lake_gradient_sample', 'lake_gradient_uv', 'lake_gradient_coarse_x',
                   'lake_response_output', 'lake_gradient_coarse_y', 'lake_gradient_fine_x', 'lake_gradient_fine_y')
WAVE_STAGES = ('lake_wave_samples_7_8', 'lake_wave_samples_9_10', 'lake_wave_uv_7_8',
               'lake_response_output', 'lake_wave_uv_9_10', 'lake_wave_weights', 'lake_wave_combined')


def displacement_shader(mode, eye, metadata):
    if metadata.get('lake_waves'):
        if metadata.get('lake_normals') or (mode == Mode.SINGLE_PASS and metadata.get('lake_inner_edge') is not True):
            raise ProtocolError('lake waves: incompatible layout or missing physical lake edge')
        if metadata.get('lake_gradients'):
            return ('3b5d1369db15e4dd1157a4ada5871dd49d75a8a4ee6618d8b9f57cb3ea133f29',
                    '4ea7d54823f8b4cab0c41d881244893550af269faccaf9056140a970da4c7798',
                    'f0fb43045ec367625a5cb57135130c2ac0dfa1d2a35a6037086487579c03802e')[eye if mode == Mode.SINGLE_PASS else 2]
        return ('681ceb7c23a8534b4533bbd3cf3a2392680a7a29b9f7263bb5458bdc645a16f4',
                '46ae8497b47facb0ed0ea0b4ff0ee10cad46db7c59c2d38c387d3eb192eada52',
                '3488a6fdb8d47b863b0fc3eec99e04519f3321197757e39008c9fb9ec0156ae3')[eye if mode == Mode.SINGLE_PASS else 2]
    if metadata.get('lake_normals') and (mode != Mode.SINGLE_PASS or metadata.get('lake_inner_edge') is True):
        return ('cabaa9a88e0fdb5f643ec03d8d2c25f3f5c45391d56986a37f509223bd4c9015', '7c2e9cd9e91b5eecedb15356f93a2def82ff4b0dfa67be25f4cba906a23973d4', '4438de5846baee18c2e132676c6bb003fe90514595e51cb13277ac4cfe01bf40')[eye if mode == Mode.SINGLE_PASS else 2]
    if mode != Mode.SINGLE_PASS:
        return 'c08313a96d5c3e4ada2cc87092ee734e95716776b90d62dcf630a3109b030b6b'
    if metadata.get('lake_inner_edge') is not True:
        raise ProtocolError("lake displacement: missing physical lake edge")
    if eye == 0 and metadata.get('lake_color_mirror') is True:
        return 'b638562431779b8a9539ad644484b3c8cf8cfb0607558c8fbd5c0c65bc876091'
    if eye == 0 and metadata.get('lake_depth_edge') is True:
        return '98164e457fb41ad1b5f11b999d11ca36493f53980d28ad41799b1087f4399dbe'
    return ('0acd25159255d5613664fa6ba9c42c7d651038ae8ec2686f16326a2a93190454', 'fbd2df56e55959a172d8e690ac2eaa8308996d7136524373910eacf3999e1714')[eye]


LAKE_INNER_EDGE_SHADER = 'b32309a2578b22bb1196640aa113f80c0311d582868d9e563ee3329b9dacf497'
LAKE_RESPONSE_SHADERS = ('c213e4dcf907b1e656e6d0617e3a34966a3e7bfc125a189f9cbdd735dbbcaefb', 'a7d4ed15d03c5de8bfdb68bfc2d0a6d45cdd0dddba1017a651122b589182483c', 'dbb738b98ffb175e51195cf007ca7d3ada04fa6577271ab520fd5ec803a83e06')
LAKE_LIGHTING_SHADERS = ('29c706a12093087514a98edf4f5f2f2d6930dc4bfca7c44e4be3906afe4c831e', '752f6213c87b9a04e5f605b4f4297bbb141afc1da20d5fa75f921e28a4ecf272', '566fb9c335d6aef2c091cbc24e067265d49c085d07f9b9e9671f1e1a4f5f9c1e')
LAKE_SAMPLE_SHADERS = ("a80e88161967cb43c970f5cf66a0554e1518d9a31f9259ccb20aed5e9b3782e8", "8732e8a0257afbbc285a9306c170dd5aae4175c88d75fce6e8df615c66a6baf5", "91d198ef55fa6eddfe0bd722f6d04a75badc8d98bd11e5289a175676a6be4608")
PLANAR_UV_SHADERS = ("b1ec7ea2909c9a1b779c2e309a23748ae00d9e58731dc0bfc52ec5b2e22fbe2c",
                     "3d730ca9c288825627b469ce533739c16db991e7391559e27936aa3bf8caf32b",
                     "8cf033e62f020701cdd3c0c7b129721d7f6a8f3796cfb4963232eb5589664a0f")
EYE_WIDTH_SHADERS = ("15845b449c8fc5f2a432569140ca2108cf23bc4807cbb8c3a33ecabc2ca02581",
                     "5f74b5bf1592c5e85ef074ffc4339125316d57b7389ecea9a21559a28a81a4e8")
MIRRORED_PLANAR_SHADERS = ("3698026ab39d3d391cd6eadd0fd7cfd13883b319372d521a4f6d9296b873b7a1",
                           "d30ee25eff4da5fe167dfe01fede1d101bf0843a613d60fbad0f1b638e24ea7f")
PROJECTED_PLANAR_SHADERS = ("c02b0d8741d2476d6cdf88295c7353b37695f7c6aeb95a8045ddb7613794cb77",
                            "7b8871205488fcdf3de5ea0154f21003f838aff6e970df08831efa7f0677e9f6")
CENTER_PLANAR_SHADERS = ("2ecbfead5ab628299d3348958618e10d54fd9a5db0e93bcb653d39a1c79b7a62",
                         "2bae625424b19584fee5548b050e2f4d268fb7c7c9bfbaeb135607179ab97637")


def require_lake_shader(identity, expected: str) -> None:
    if (not valid_mapped_identity(identity) or identity["identity"]["pristine_sha256"] != LAKE_SHADER or
            identity["identity"]["actual_sha256"] != expected):
        raise ProtocolError(f"visual protocol: expected lake shader {expected}, got {identity}")


def valid_mapped_identity(identity) -> bool:
    return (isinstance(identity, dict) and identity.get("status") == "mapped" and
            isinstance(identity.get("identity"), dict) and
            all(isinstance(identity["identity"].get(key), str) and
                len(identity["identity"][key]) == 64 and
                all(ch in "0123456789abcdef" for ch in identity["identity"][key])
                for key in ("pristine_sha256", "actual_sha256")))


def validate_eye_width_producers(info, metadata, capture_id):
    projection = info.get("projection") or {}
    producers = projection.get("producers") or []
    width, height = metadata["eye_width"], metadata["eye_height"]
    if projection.get("camera_size") != [2 * width, height] or [p.get("pass") for p in producers] != [10, 11, 12]:
        raise ProtocolError("visual protocol: expected the packed source camera and three reflection passes")
    for p in producers:
        writer = p["pass"] in (11, 12)
        matrix = p.get("projection_f") or []
        if (p.get("frame") != metadata["predicted_display_time_ns"] or p.get("capture_id") != capture_id or
                p.get("dispatch_eye") != 0 or not p.get("manager_camera") or p.get("source_camera") != p["manager_camera"] or
                not p.get("snapshot_camera") or p["snapshot_camera"] == p["source_camera"] or
                len(matrix) != 16 or abs(matrix[0] - matrix[5] * height / (width if writer else 2 * width)) > 1e-6 or
                (writer and info["resource"] not in p.get("target_resources", []))):
            raise ProtocolError(f"visual protocol: unexpected reflection producer context or target on pass {p['pass']}")


def validate_water_inputs(folder: Path, capture_id: str, metadata: dict, mode: Mode,
                          shader_identity: bool = False, water_tess_draw: int | None = None, water_surface_inputs: bool = False) -> None:
    slots = {"water_input_reflection": ("reflection", 1),
             "water_input_reflection_alpha": ("reflection_alpha", 12),
             "water_input_refraction": ("refraction", 3)}
    if water_tess_draw is not None:
        slots["water_input_static_reflection"] = ("static_reflection", 2)
    surface = {f"water_input_{name}": (name, index) for name, index in (
        ("refraction_depth", 0), ("wave0", 7), ("wave1", 8), ("wave2", 9), ("wave3", 10), ("ambient_occlusion", 11))}
    if water_surface_inputs:
        slots.update(surface)
    records = metadata.get("stages")
    if not isinstance(records, list):
        raise ProtocolError("visual protocol: requested water inputs have no stage records")
    expected = {(stage, eye) for stage in slots for eye, required in enumerate(mode.rendered_eyes) if required}
    seen = set()
    for record in records:
        if not isinstance(record, dict):
            raise ProtocolError("visual protocol: invalid water input stage record")
        stage = record.get("stage")
        if not isinstance(stage, str):
            raise ProtocolError("visual protocol: invalid stage name")
        if stage not in slots:
            continue
        eye = record.get("eye")
        if type(eye) is not int or (stage, eye) not in expected or (stage, eye) in seen:
            raise ProtocolError("visual protocol: unexpected or duplicate water input stage")
        seen.add((stage, eye))
        info = record.get("water_input")
        source = record.get("source")
        if (not isinstance(info, dict) or info.get("capture_id") != capture_id or
                info.get("slot") != slots[stage][0] or info.get("shader_slot") != slots[stage][1] or
                type(info.get("pixel_shader")) is not int or info["pixel_shader"] <= 0 or
                type(info.get("draw_index")) is not int or info["draw_index"] < 0 or
                type(info.get("tessellated")) is not bool or
                any(type(info.get(key)) is not int for key in
                    ("view_format", "view_dimension", "most_detailed_mip", "mip_levels", "resource", "resource_format")) or
                type(info.get("resource")) is not int or info["resource"] <= 0 or
                info["view_dimension"] != 4 or info["most_detailed_mip"] < 0 or info["mip_levels"] <= 0 or
                record.get("error") is not None or not isinstance(source, dict)):
            raise ProtocolError("visual protocol: invalid requested water input metadata")
        if water_tess_draw is not None and (not info["tessellated"] or type(info.get("tess_index")) is not int or info["tess_index"] != water_tess_draw or record.get("frame") != metadata.get("predicted_display_time_ns")):
            raise ProtocolError("visual protocol: water draw selection mismatch")
        if metadata.get('lake_waves'):
            if stage.startswith('water_input_wave') and info.get('sampler') != [info['shader_slot'], 85, 1, 1, 1]:
                raise ProtocolError(f"lake waves: unexpected selected sampler for {stage}: {info.get('sampler')}")
            if stage == 'water_input_static_reflection':
                coeff = (info.get('projection') or {}).get('ps_wave_coefficients')
                if not isinstance(coeff, list) or len(coeff) != 4 or not all(type(v) in (int, float) and math.isfinite(v) for v in coeff):
                    raise ProtocolError('lake waves: missing PS blend coefficients')
        if stage == 'water_input_static_reflection':
            if metadata.get('lake_producer'):
                producer = (info.get('projection') or {}).get('wave_producer') or {}
                for stage_name in ('vs','hs','ds'):
                    digest = producer.get(stage_name)
                    if not isinstance(digest,str) or len(digest)!=64 or any(c not in '0123456789abcdef' for c in digest):
                        raise ProtocolError(f'lake producer: missing bound {stage_name} identity')
            binding = (info.get('projection') or {}).get('ds_mvp_binding')
            treated = metadata.get('lake_ds_mvp')
            if treated is not None:
                generated = physical_water_mvp(metadata, eye)
                if (mode != Mode.SINGLE_PASS or not isinstance(binding, dict) or
                        binding.get('generated') != generated or
                        not (0 < binding['vs_pointer'] != binding['ds_pointer'] > 0) or
                        len(binding['original']) != 40 or
                        binding['actual'] != (generated + binding['original'][16:] if treated else binding['original'])):
                    raise ProtocolError('lake DS MVP: selected draw differs from requested matrix or original suffix')
            elif binding is not None:
                raise ProtocolError('lake DS MVP: unexpected buffer substitution')
        identity = info.get("shader_identity")
        if shader_identity and not valid_mapped_identity(identity):
            raise ProtocolError("visual protocol: water pixel shader identity is unmapped or incomplete")
        formats = ((26, 26), (41, 41), (39, 41), (10, 10), (28, 28)) if stage in surface else ((26, 26), (41, 41), (39, 41))
        if water_surface_inputs or (stage == "water_input_static_reflection" and shader_identity and
                                    metadata.get("water_reflection") is None and water_tess_draw == 7):
            expected_shader = PACKED_LAKE_SHADER if mode == Mode.SINGLE_PASS else LAKE_SHADER
            if metadata.get("water_reflection") in ("lake_samples", "lake_lighting", "lake_sky", "lake_sky_buffer", "lake_response", "lake_near_patch", "lake_tess_patch", "lake_patch_draw", "lake_patch_owners", "lake_camera_control", "lake_domain_camera", "lake_hull_camera"):
                expected_shader = (LAKE_SAMPLE_SHADERS if metadata["water_reflection"] == "lake_samples" else LAKE_RESPONSE_SHADERS if metadata["water_reflection"] in ("lake_response", "lake_near_patch", "lake_tess_patch", "lake_patch_draw", "lake_patch_owners", "lake_camera_control", "lake_domain_camera", "lake_hull_camera") else LAKE_LIGHTING_SHADERS)[eye if mode == Mode.SINGLE_PASS else 2]
                if mode == Mode.SINGLE_PASS and stage == "water_input_static_reflection":
                    validate_eye_width_producers(info, metadata, capture_id)
            if metadata.get("water_reflection") == "eye_width_planar":
                expected_shader = EYE_WIDTH_SHADERS[eye]
                if stage == "water_input_static_reflection":
                    validate_eye_width_producers(info, metadata, capture_id)
            if eye == 0 and metadata.get('lake_inner_edge') is True:
                expected_shader = LAKE_INNER_EDGE_SHADER
            if metadata.get("lake_displacement"):
                expected_shader = displacement_shader(mode, eye, metadata)
            require_lake_shader(identity, expected_shader)
        if type(source.get("format_id")) is not int or source["format_id"] != info["view_format"] or (info["resource_format"], info["view_format"]) not in formats:
            raise ProtocolError("visual protocol: water input source format disagrees with its view")
        width, height, pixel_bytes = (source.get(key) for key in ("width", "height", "bytes_per_pixel"))
        if any(type(value) is not int or value <= 0 for value in (width, height, pixel_bytes)):
            raise ProtocolError("visual protocol: invalid water input dimensions or format")
        raw = record.get("raw")
        if not isinstance(raw, str):
            raise ProtocolError("visual protocol: requested water input has no raw artifact")
        try:
            path = artifact_path(folder / f"{capture_id}.capture.json", raw)
            if path.stat().st_size != width * height * pixel_bytes:
                raise ProtocolError("visual protocol: water input artifact size mismatch")
            if metadata.get('lake_gradients') and stage in ('water_input_wave1','water_input_wave2'):
                fp = info.get('wave_footprint') or {}
                mip = fp.get('mip_sha256') or []
                lod = fp.get('lod_bits') or []
                if (len(mip) != 8 or any(not isinstance(h,str) or len(h)!=64 or any(c not in '0123456789abcdef' for c in h) for h in mip) or
                        mip[0] != hashlib.sha256(path.read_bytes()).hexdigest() or len(lod)!=4 or
                        any(type(v) is not int or not 0<=v<2**32 for v in lod) or
                        type(fp.get('max_anisotropy')) is not int or not 1<=fp['max_anisotropy']<=16):
                    raise ProtocolError(f'lake gradients: invalid measured mip or sampler state for {stage}')
        except (OSError, ValueError) as error:
            raise ProtocolError(f"visual protocol: invalid water input artifact: {error}") from error
    if seen != expected:
        raise ProtocolError("visual protocol: requested water input stages are missing")


def validate_sun_inputs(folder: Path, capture_id: str, metadata: dict, mode: Mode) -> None:
    stages = {"pre_sun_color", "pre_sun_depth"}
    records = metadata.get("stages")
    if not isinstance(records, list):
        raise ProtocolError("visual protocol: requested sun inputs have no stage records")
    expected = {(stage, eye) for stage in stages for eye, required in enumerate(mode.rendered_eyes) if required}
    seen = set()
    for record in records:
        if not isinstance(record, dict) or not isinstance(record.get("stage"), str):
            raise ProtocolError("visual protocol: invalid sun input stage record")
        stage = record["stage"]
        if stage not in stages:
            continue
        eye = record.get("eye")
        if type(eye) is not int or (stage, eye) not in expected or (stage, eye) in seen:
            raise ProtocolError("visual protocol: unexpected or duplicate sun input stage")
        seen.add((stage, eye))
        info, source = record.get("sun_input"), record.get("source")
        if (not isinstance(info, dict) or info.get("capture_id") != capture_id or
                any(type(info.get(key)) is not int for key in
                    ("view", "resource", "view_format", "view_dimension", "mip")) or
                info["view"] <= 0 or info["resource"] <= 0 or info["mip"] != 0 or
                record.get("error") is not None or not isinstance(source, dict)):
            raise ProtocolError("visual protocol: invalid requested sun input metadata")
        source_format = source.get("format_id")
        if type(source_format) is not int:
            raise ProtocolError("visual protocol: invalid sun input source format")
        valid_format = ((info["view_format"] == 26 and source_format == 26) if stage == "pre_sun_color"
                        else (info["view_format"], source_format) in ((20, 19), (20, 20), (40, 39), (40, 40), (45, 44), (45, 45), (55, 53), (55, 55)))
        if not valid_format:
            raise ProtocolError("visual protocol: unsupported sun input resource/view format combination")
        width, height, pixel_bytes = (source.get(key) for key in ("width", "height", "bytes_per_pixel"))
        if any(type(value) is not int or value <= 0 for value in (width, height, pixel_bytes)):
            raise ProtocolError("visual protocol: invalid sun input dimensions or format")
        raw = record.get("raw")
        if not isinstance(raw, str):
            raise ProtocolError("visual protocol: requested sun input has no raw artifact")
        try:
            path = artifact_path(folder / f"{capture_id}.capture.json", raw)
            if path.stat().st_size != width * height * pixel_bytes:
                raise ProtocolError("visual protocol: sun input artifact size mismatch")
        except (OSError, ValueError) as error:
            raise ProtocolError(f"visual protocol: invalid sun input artifact: {error}") from error
    if seen != expected:
        raise ProtocolError("visual protocol: requested sun input stages are missing")


def validate_png(path):
    data = path.read_bytes()
    if len(data) > 128 * 1024 * 1024 or data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SessionError(f"capture: invalid PNG signature or size: {path}")
    offset = 8
    header = None
    compressed = bytearray()
    ended = False
    idat_closed = False
    while offset < len(data):
        if offset + 12 > len(data):
            raise SessionError("capture: truncated PNG chunk")
        size, kind = struct.unpack_from(">I4s", data, offset)
        end = offset + 12 + size
        if end > len(data):
            raise SessionError("capture: truncated PNG payload")
        payload = data[offset + 8:end - 4]
        crc = struct.unpack_from(">I", data, end - 4)[0]
        if zlib.crc32(kind + payload) & 0xffffffff != crc:
            raise SessionError("capture: PNG chunk CRC mismatch")
        if header is None and kind != b"IHDR":
            raise SessionError("capture: PNG does not begin with IHDR")
        if kind == b"IHDR":
            if header is not None or size != 13:
                raise SessionError("capture: invalid PNG IHDR")
            header = struct.unpack(">IIBBBBB", payload)
        elif kind == b"IDAT":
            if idat_closed:
                raise SessionError("capture: nonconsecutive PNG IDAT chunks")
            compressed.extend(payload)
        elif kind == b"IEND":
            if size or end != len(data):
                raise SessionError("capture: invalid PNG IEND or trailing bytes")
            ended = True
            break
        else:
            if compressed:
                idat_closed = True
            if kind[:1].isupper():
                raise SessionError(f"capture: unsupported critical PNG chunk {kind!r}")
        offset = end
    if header is None or not ended or not compressed:
        raise SessionError("capture: PNG is incomplete")
    width, height, depth, color, compression, filtering, interlace = header
    if not (0 < width <= 16384 and 0 < height <= 16384) or depth != 8 or color not in (2, 6) or (compression, filtering, interlace) != (0, 0, 0):
        raise SessionError("capture: unsupported PNG layout")
    stride = width * (3 if color == 2 else 4) + 1
    expected = stride * height
    if expected > 256 * 1024 * 1024:
        raise SessionError("capture: PNG decoded image is too large")
    decoder = zlib.decompressobj()
    try:
        pixels = decoder.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise SessionError(f"capture: corrupt PNG deflate stream: {error}") from error
    if len(pixels) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise SessionError("capture: PNG decoded length or zlib stream is invalid")
    if any(pixels[row * stride] > 4 for row in range(height)):
        raise SessionError("capture: invalid PNG row filter")
    return width, height
