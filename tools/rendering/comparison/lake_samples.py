import argparse
import json
from pathlib import Path

import numpy as np

from tools.rendering.capture.validation import (
    validate_water_inputs,
    validate_water_treatment,
)
from tools.rendering.comparison.images import Manifest, Mode, read_json
from tools.rendering.comparison.planar_uv import mirror, sample_map
from tools.rendering.comparison.stages import (
    DxgiFormat,
    Source,
    Stage,
    artifact_path,
    decode_raw,
    load_stage,
    load_water_mask,
    validated_samples,
)
from tools.rendering.protocol import Mode as ProtocolMode


def load(root, mode, spec, sample, eye, lighting=False, response=False):
    path = root / spec.metadata
    wire = read_json(path)
    if wire.get("water_reflection") not in (
        ("lake_response", "lake_near_patch", "lake_tess_patch", "lake_patch_draw", "lake_patch_owners", "lake_camera_control", "lake_domain_camera", "lake_hull_camera")
        if response
        else ("lake_lighting", "lake_sky", "lake_sky_buffer")
        if lighting
        else ("lake_samples",)
    ):
        raise ValueError(f"{path}: not a lake sample trace")
    validate_water_treatment(
        path.parent, sample.capture_id, wire, ProtocolMode(mode.value)
    )
    validate_water_inputs(
        path.parent, sample.capture_id, wire, ProtocolMode(mode.value), True, 7, True
    )

    def raw(stage):
        (rec,) = [r for r in wire["stages"] if r["stage"] == stage and r["eye"] == eye]
        return artifact_path(path, rec["raw"]).read_bytes(), rec["source"]

    def trace(name, displacement=False):
        data, src = raw(
            (
                "lake_normal_" if displacement and wire.get("lake_normals") else "lake_displacement_" if displacement else "lake_response_"
                if response
                else "lake_lighting_"
                if lighting
                else "lake_sample_"
            )
            + name
        )
        array = np.frombuffer(data, "<f4").reshape(src["height"], src["width"], 4)
        if mode == Mode.SINGLE_PASS:
            halves = np.split(array, 2, axis=1)
            if np.any(halves[1 - eye]):
                raise ValueError(f"{path}: trace wrote outside selected eye")
            array = halves[eye]
        if not np.isfinite(array).all():
            raise ValueError(f"{path}: nonfinite sample trace")
        return array

    if response:
        fields = {n: trace(n) for n in ("color", "uv", "tint", "output", "air")}
        if wire.get("lake_normals"):
            fields['mesh'], fields['mixed'] = [trace(k, displacement=True) for k in ('mesh','mixed')]
            rec, = [r for r in wire['stages'] if r['stage']=='water_input_static_reflection' and r['eye']==eye]
            projection = rec['water_input']['projection']
            fields['view_xy'] = np.array(projection['ps_refraction_view'])[:, :2]
            fields['strength'] = projection['ps_type_reflection'][2]
            if any(np.any(fields[k][fields['output'][...,3]==0]) for k in ('mesh','mixed')):
                raise ValueError(f'{path}: normal trace escaped selected draw')
        elif wire.get("lake_displacement"):
            for name in ("depth", "surface"):
                fields[name] = trace(name, displacement=True)
                if np.any(fields[name][fields["output"][..., 3] == 0]):
                    raise ValueError(f"{path}: displacement escaped the selected draw")
            data, src = raw("water_input_refraction_depth")
            fields["depth_tex"] = np.frombuffer(data, '<f4').reshape(src['height'], src['width'])
        data, src = raw("water_input_refraction")
        fields["tex"] = decode_raw(data, Source.from_wire(src))[1]
        fields["depth_bounded"] = eye == 0 and wire.get("lake_depth_edge") is True
        fields["packed"] = mode == Mode.SINGLE_PASS

        def stage_plane(name):
            key = 0 if fields["packed"] else eye
            (rec,) = [
                r for r in wire["stages"] if r["stage"] == name and r["eye"] == key
            ]
            if rec["frame"] != wire["predicted_display_time_ns"] or rec.get("error"):
                raise ValueError(f"{path}: invalid upstream stage {name}")
            src = Source.from_wire(rec["source"])
            bits, values, _ = decode_raw(artifact_path(path, rec["raw"]).read_bytes(), src)
            if name == "geometry_depth":
                if src.format not in (DxgiFormat.R32G8X24_TYPELESS, DxgiFormat.D32_FLOAT_S8X24_UINT):
                    raise ValueError(f"{path}: no geometry stencil plane")
                fields["stencil"] = bits[..., 4:5].copy()
            return values

        fields["upstream"] = {
            n: stage_plane(n)
            for n in (
                "post_resolve",
                "post_sun",
                "post_atmosphere",
                "water_before",
                "gbuffer0",
                "gbuffer1",
                "gbuffer2",
                "gbuffer3",
                "geometry_depth",
            )
        }
        if any(rec["stage"] == "before_z_pass_depth" for rec in wire["stages"]):
            fields["early_depth"] = {name: stage_plane(name) for name in (
                "before_z_pass_depth", "after_z_pass_depth", "after_z_velocity_depth")}
        if any(rec["stage"] == "resolve_world" for rec in wire["stages"]):
            def resolve_trace(name):
                data, src = raw("resolve_" + name)
                arr = np.frombuffer(data, "<f4").reshape(src["height"], src["width"], 4)
                if mode == Mode.SINGLE_PASS:
                    halves = np.split(arr, 2, axis=1)
                    if np.any(halves[1-eye]):
                        raise ValueError(f"{path}: resolve trace escaped physical eye")
                    arr = halves[eye]
                if not np.isfinite(arr).all():
                    raise ValueError(f"{path}: nonfinite resolve trace")
                return arr

            fields["deferred"] = {n: resolve_trace(n) for n in ("world", "view", "half_samples", "full_samples")}
            mark = fields["deferred"]["world"][..., 3]
            if not np.isin(mark, [0, 1]).all() or not np.array_equal(mark, fields["deferred"]["view"][..., 3]):
                raise ValueError("resolve trace: invalid coverage")
        if wire.get("water_reflection") in ("lake_near_patch", "lake_tess_patch", "lake_patch_draw", "lake_patch_owners", "lake_camera_control", "lake_domain_camera", "lake_hull_camera"):
            fields["bracket"] = {
                (boundary, plane): stage_plane(f"{boundary}_{('tess_patch' if wire['water_reflection'] in ('lake_patch_draw', 'lake_patch_owners', 'lake_camera_control', 'lake_domain_camera', 'lake_hull_camera') else wire['water_reflection'].removeprefix('lake_'))}_{plane}")
                for boundary in ("before", "after") for plane in ("depth", "albedo")
            }
        if wire.get("water_reflection") == "lake_patch_owners":
            key = 0 if fields["packed"] else eye
            (rec,) = [r for r in wire["stages"] if r["stage"] == "patch_depth_owners" and r["eye"] == key]
            owners = np.fromfile(artifact_path(path, rec["raw"]), "<u4").reshape(rec["source"]["height"], -1)
            if owners.max() > 256:
                raise ValueError(f"{path}: invalid depth owner ordinal")
            fields["owners"] = np.split(owners, 2, axis=1)[eye] if fields["packed"] else owners
        if wire.get("water_reflection") in ("lake_camera_control", "lake_domain_camera", "lake_hull_camera"):
            fields["draw_bracket"] = {
                (boundary, plane): stage_plane(f"{boundary}_patch_draw_{plane}")
                for boundary in ("before", "after") for plane in ("depth", "albedo")
            }
        if not np.array_equal(
            fields["tex"], fields["upstream"]["water_before"]
        ) or not np.array_equal(fields["tex"], fields["upstream"]["post_atmosphere"]):
            raise ValueError(f"{path}: refraction texture differs from upstream color")
        marked = fields["output"][:, :, 3]
        if not np.array_equal(marked, fields["color"][:, :, 3]):
            raise ValueError("response color coverage differs from shader output")
    elif lighting:
        fields = {n: trace(n) for n in ("base", "mixed", "lit", "output")}
        marked = fields["output"][:, :, 3]
    else:
        uv, depth, color, planar = map(trace, ("uv", "depth", "color", "planar"))
        data, src = raw("water_input_refraction")
        texture = decode_raw(data, Source.from_wire(src))[1]
        data, src = raw("water_input_refraction_depth")
        z = np.frombuffer(data, "<f4").reshape(src["height"], src["width"])
        marked = color[:, :, 3]
    if not np.isin(marked, (0, 1)).all():
        raise ValueError(f"{path}: invalid write marker")
    mask = load_water_mask(root, mode, spec, sample, eye).water & (marked == 1)
    if not mask.any():
        raise ValueError(f"{path}: no selected draw pixels")
    shaded = load_stage(
        root,
        mode,
        spec,
        sample,
        (Stage.WATER_AFTER_LEFT, Stage.WATER_AFTER_RIGHT)[eye],
        eye,
    ).values
    if lighting or response:
        return dict(mask=mask, shaded=shaded, **fields)
    return dict(
        mask=mask,
        uv=uv,
        z=depth,
        color=color,
        planar=planar,
        tex=texture,
        depth_tex=z,
        shaded=shaded,
        packed=mode == Mode.SINGLE_PASS,
    )


def metrics(a, b, eye):
    mask = a["mask"] & b["mask"]
    if not mask.any():
        raise ValueError("lake samples: no common marked pixels")

    def mae(x, y):
        return float(np.abs(x.astype("d") - y).mean())

    def native(uv, s):
        xy = uv.copy()
        xy[:, 0] = xy[:, 0] * 2 - eye if s["packed"] else xy[:, 0]
        return xy

    def encoded(uv, s):
        xy = uv.copy()
        xy[:, 0] = (xy[:, 0] + eye) / 2 if s["packed"] else xy[:, 0]
        return xy

    uv, vb = (s["uv"][mask] for s in (a, b))
    u, v = native(uv[:, :2], a), native(vb[:, :2], b)
    ca, cb = (s["color"][mask, :3] for s in (a, b))
    pa, pb = (s["planar"][mask] for s in (a, b))
    za, zb = (s["z"][mask] for s in (a, b))
    sample_at_a = sample_map(b["tex"], encoded(u, b))
    sample_at_b = sample_map(b["tex"], vb[:, :2])
    expected_a = sample_map(a["tex"], uv[:, :2])
    h, w = mask.shape
    indices = np.column_stack(np.nonzero(mask)[::-1])
    base = (indices + 0.5) / [w, h]

    def point(z, xy):
        height, width = z.shape
        ix, iy = np.clip(
            np.floor(xy * [width, height]).astype(int), 0, [width - 1, height - 1]
        ).T
        return z[iy, ix]

    return dict(
        common=int(mask.sum()),
        shade=mae(a["shaded"][mask], b["shaded"][mask]),
        planar=mae(pa[:, :3], pb[:, :3]),
        ao=mae(pa[:, 3], pb[:, 3]),
        planar_uv=mae(uv[:, 2:], vb[:, 2:]),
        color=mae(ca, cb),
        color_uv_pixels=np.abs((u - v) * [w, h]).mean(0).tolist(),
        depth_uv_pixels=np.abs((native(za[:, :2], a) - native(zb[:, :2], b)) * [w, h])
        .mean(0)
        .tolist(),
        base_depth=mae(za[:, 2], zb[:, 2]),
        candidate_depth=mae(za[:, 3], zb[:, 3]),
        model_color_a=mae(ca, expected_a),
        model_color_b=mae(cb, sample_at_b),
        correct_uv_color=mae(ca, sample_at_a),
        color_change_from_uv=mae(sample_at_a, sample_at_b),
        outside_eye=int(np.any((v < 0) | (v > 1), axis=1).sum()),
        base_depth_model=[
            mae(s["z"][mask, 2], point(s["depth_tex"], encoded(base, s)))
            for s in (a, b)
        ],
        candidate_depth_model=[
            mae(s["z"][mask, 3], point(s["depth_tex"], s["z"][mask, :2]))
            for s in (a, b)
        ],
    )


def residual(a, b, eye):
    mask = a["mask"] & b["mask"]
    y, x = np.nonzero(mask)
    h, w = mask.shape
    base = np.column_stack((x + 0.5, y + 0.5))

    def displacement(s, key):
        uv = s[key][mask, :2].astype("d")
        if s["packed"]:
            uv[:, 0] = uv[:, 0] * 2 - eye
        return uv * [w, h] - base

    def rgb_delta(key):
        return b[key][mask, :3].astype("d") - a[key][mask, :3]

    def retention(s):
        candidate = displacement(s, "z")
        final = displacement(s, "uv")
        length_sq = (candidate * candidate).sum(axis=1)
        return np.divide(
            (candidate * final).sum(axis=1),
            length_sq,
            out=np.full(len(x), np.nan),
            where=length_sq > 0.01**2,
        )

    shade, color, planar = map(rgb_delta, ("shaded", "color", "planar"))
    err = np.abs(shade).mean(axis=1)
    candidate_a, candidate_b = (displacement(s, "z") for s in (a, b))
    final_a, final_b = (displacement(s, "uv") for s in (a, b))
    ra, rb = retention(a), retention(b)
    loss = err.sum()

    def subset(m):
        return dict(
            pixels=int(m.sum()),
            error_fraction=float(err[m].sum() / loss) if loss else None,
            shade=float(err[m].mean()) if m.any() else None,
            color=float(np.abs(color[m]).mean()) if m.any() else None,
            planar=float(np.abs(planar[m]).mean()) if m.any() else None,
        )

    order = np.argsort(-err, kind="stable")
    top = np.zeros(len(x), dtype=bool)
    top[order[: (len(x) + 99) // 100]] = True
    k = order[:8]
    return dict(
        common=len(x),
        all=subset(np.ones_like(top)),
        top_percent=subset(top),
        remainder=subset(~top),
        zero_color_planar=subset(
            ~(np.any(color != 0, axis=1) | np.any(planar != 0, axis=1))
        ),
        shade_exceeds_both=subset(err > (np.abs(color) + np.abs(planar)).mean(axis=1)),
        unresolvable_retention=subset(~(np.isfinite(ra) & np.isfinite(rb))),
        retention_delta_gt_001=subset(np.abs(ra - rb) > 0.01),
        final_displacement_delta_gt_tenth_pixel=subset(
            np.linalg.norm(final_b - final_a, axis=1) > 0.1
        ),
        candidate_displacement_delta_gt_tenth_pixel=subset(
            np.linalg.norm(candidate_b - candidate_a, axis=1) > 0.1
        ),
        worst=[
            dict(
                x=int(x[i]),
                y=int(y[i]),
                ref=a["shaded"][mask][i].tolist(),
                test=b["shaded"][mask][i].tolist(),
                shade=shade[i].tolist(),
                color=color[i].tolist(),
                planar=planar[i].tolist(),
                ref_retention=float(ra[i]) if np.isfinite(ra[i]) else None,
                test_retention=float(rb[i]) if np.isfinite(rb[i]) else None,
            )
            for i in k
        ],
    )


def lighting_metrics(a, b, eye):
    mask = a["mask"] & b["mask"]
    if not mask.any():
        raise ValueError("lighting trace: empty common mask")
    d = {
        k: b[k][mask].astype("d") - a[k][mask]
        for k in ("base", "mixed", "lit", "output", "shaded")
    }
    err = np.abs(d["output"][:, :3]).mean(axis=1)
    order = np.argsort(-err, kind="stable")
    n = (len(order) + 99) // 100

    def group(sel):
        def mae(c):
            return float(np.abs(c[sel]).mean())

        return dict(
            pixels=len(sel),
            **{k: mae(v[:, :3]) for k, v in d.items()},
            weight=mae(d["base"][:, 3]),
            shadow=mae(d["mixed"][:, 3]),
            foam=mae(d["lit"][:, 3]),
            mix_increment=mae(d["mixed"][:, :3] - d["base"][:, :3]),
            lighting_increment=mae(d["lit"][:, :3] - d["mixed"][:, :3]),
            air_increment=mae(d["output"][:, :3] - d["lit"][:, :3]),
            target_increment=mae(d["shaded"] - d["output"][:, :3]),
        )

    return dict(
        common=len(order),
        all=group(order),
        top_percent=group(order[:n]),
        remainder=group(order[n:]),
    )


def source_footprint(a, b, eye, uv, gain, resolve):
    names = ("gbuffer0", "gbuffer1", "gbuffer2", "gbuffer3", "geometry_depth", "stencil")

    def plane(s, name):
        src = s["bracket"][name] if isinstance(name, tuple) else s["stencil"] if name == "stencil" else s["upstream"][name]
        return np.split(src, 2, axis=1)[eye] if s["packed"] else src

    ra, rb = (plane(s, "post_resolve") for s in (a, b))
    if ra.shape != rb.shape or ra.shape[:2] != a["mask"].shape:
        raise ValueError("source footprint: resolve shape mismatch")
    h, w = ra.shape[:2]
    flags = np.zeros((h, w), dtype=np.uint8)
    for i, name in enumerate(names):
        x, y = plane(a, name), plane(b, name)
        if x.shape != y.shape or x.shape[:2] != (h, w):
            raise ValueError(f"source footprint: {name} shape mismatch")
        flags |= np.any(x != y, axis=2).astype(np.uint8) << i

    xy = np.clip(mirror(uv) * [w, h] - 0.5, 0, [w - 1, h - 1])
    x0, y0 = np.floor(xy).astype(int).T
    fx, fy = (xy - np.column_stack((x0, y0))).T
    delta = rb - ra
    signed = np.zeros((h, w))
    absolute = np.zeros((h, w))
    visits = np.zeros((h, w), dtype=int)
    reconstructed = np.zeros_like(resolve)
    out_flags = np.zeros(len(uv), dtype=np.uint8)
    for y, wy in ((y0, 1 - fy), (np.minimum(y0 + 1, h - 1), fy)):
        for x, wx in ((x0, 1 - fx), (np.minimum(x0 + 1, w - 1), fx)):
            weight = (wx * wy)[:, None]
            contribution = weight * delta[y, x]
            reconstructed += contribution
            weighted = contribution * gain
            np.add.at(signed, (y, x), (weighted * np.sign(resolve * gain)).mean(1))
            np.add.at(absolute, (y, x), np.abs(weighted).mean(1))
            live = weight[:, 0] > 0
            np.add.at(visits, (y[live], x[live]), 1)
            out_flags[live] |= flags[y[live], x[live]]
    if not np.allclose(reconstructed, resolve, atol=1e-13, rtol=1e-12):
        raise ValueError("source footprint: resolve reconstruction differs")
    used = visits > 0
    mass = np.abs(gain * resolve).mean(1)
    total = float(mass.sum())
    if not np.isclose(signed.sum(), total, atol=1e-10, rtol=1e-12):
        raise ValueError("source footprint: contributions do not sum to weighted resolve error")

    def label(code):
        return "+".join(n for i, n in enumerate(names) if code & (1 << i)) or "equal"

    def source_group(mask):
        return dict(texels=int(mask.sum()), visits=int(visits[mask].sum()),
                    signed_mass=float(signed[mask].sum()), abs_mass=float(absolute[mask].sum()))

    def by_code(classes, active, values):
        return {label(int(code)): values(active & (classes == code)) for code in np.unique(classes[active])}

    bracket = {}
    if "bracket" in b:
        for channel, final in (("depth", "geometry_depth"), ("albedo", "gbuffer0")):
            before = [plane(s, ("before", channel)) for s in (a, b)]
            after = [plane(s, ("after", channel)) for s in (a, b)]
            end = [plane(s, final) for s in (a, b)]
            bits = (("entered_different", before[0], before[1]),
                    ("a_changed", before[0], after[0]), ("b_changed", before[1], after[1]),
                    ("left_different", after[0], after[1]),
                    ("a_changed_later", after[0], end[0]), ("b_changed_later", after[1], end[1]))
            codes = np.zeros_like(flags)
            for i, (_, x, y) in enumerate(bits):
                codes |= np.any(x != y, axis=2).astype(np.uint8) << i
            bracket[channel] = {"+".join(n for i, (n, _, _) in enumerate(bits) if code & (1 << i)) or "unchanged": source_group(used & (codes == code)) for code in np.unique(codes[used])}
    draw_coverage = {}
    if "draw_bracket" in b:
        for plane_name in ("depth", "albedo"):
            changed = []
            for shot in (a, b):
                x, y = (shot["draw_bracket"][boundary, plane_name] for boundary in ("before", "after"))
                if shot["packed"]:
                    x, y = (np.split(v, 2, axis=1)[eye] for v in (x, y))
                changed.append(np.any(x != y, axis=2))
            left, right = changed
            draw_coverage[plane_name] = dict(a=int(left.sum()), b=int(right.sum()),
                a_only=source_group(used & left & ~right), b_only=source_group(used & right & ~left),
                both=source_group(used & left & right), neither=source_group(used & ~left & ~right))
    early = {}
    if "early_depth" in a and "early_depth" in b:
        previous = None
        for name in (*a["early_depth"], "before_tess_patch_depth"):
            planes = []
            for shot in (a, b):
                v = shot["bracket"]["before", "depth"] if name == "before_tess_patch_depth" else shot["early_depth"][name]
                planes.append(np.split(v, 2, axis=1)[eye] if shot["packed"] else v)
            different = np.any(planes[0] != planes[1], axis=2)
            row = {"different": source_group(used & different)}
            if previous is not None:
                old_diff = np.any(previous[0] != previous[1], axis=2)
                row["new_difference"] = source_group(used & different & ~old_diff)
                row["resolved_difference"] = source_group(used & ~different & old_diff)
                for key, old, new in zip(("a_changed", "b_changed"), previous, planes):
                    row[key] = source_group(used & np.any(old != new, axis=2))
            early[name] = row
            previous = planes
    owners = {}
    for key, shot in (("a", a), ("b", b)):
        if "owners" in shot:
            atlas = shot["owners"]
            owners[key] = {str(o): source_group(used & (atlas == o)) for o in np.unique(atlas[used])}
    deferred = {}
    if "deferred" in a and "deferred" in b:
        da, db = (shot["deferred"] for shot in (a, b))
        equal = used & (flags == 0)
        observed = (da["world"][..., 3] == 1) & (db["world"][..., 3] == 1)
        deferred["equal_unobserved"] = source_group(equal & ~observed)
        deferred["equal_observed"] = source_group(equal & observed)
        changed = []
        for key, name, channels in (("reflection", "half_samples", slice(0,3)), ("reflection_alpha", "half_samples", slice(3,4)),
                                    ("skin", "full_samples", slice(0,3)), ("shadow", "full_samples", slice(3,4)),
                                    ("world", "world", slice(0,3)), ("view", "view", slice(0,3))):
            delta = db[name][...,channels].astype("d") - da[name][...,channels]
            differs = np.any(delta != 0, axis=2)
            changed.append(differs)
            subset = equal & observed
            deferred[key] = dict(**source_group(subset & differs),
                                 delta_min=delta[subset].min(0).tolist() if subset.any() else [],
                                 delta_max=delta[subset].max(0).tolist() if subset.any() else [])
        bits = sum(v.astype(np.uint8) << i for i,v in enumerate(changed))
        deferred["joint"] = {str(int(code)):source_group(equal & observed & (bits == code)) for code in np.unique(bits[equal & observed])}
    highest = np.argsort(-signed[used], kind="stable")[:20]
    points = np.column_stack(np.nonzero(used))[highest]
    return dict(weighted_resolve_sum=total, deferred=deferred, bracket=bracket, early_depth=early, depth_owners=owners, draw_coverage=draw_coverage, source_classes=by_code(flags, used, source_group),
                pixel_classes=by_code(out_flags, np.ones_like(out_flags, dtype=bool),
                                      lambda mask: dict(pixels=int(mask.sum()), mass=float(mass[mask].sum()))),
                highest_source_texels=[dict(x=int(x), y=int(y), changes=label(int(flags[y, x])),
                                            visits=int(visits[y, x]), signed_mass=float(signed[y, x]),
                                            a={n: plane(a, n)[y, x].tolist() for n in (*names, "post_resolve")},
                                            b={n: plane(b, n)[y, x].tolist() for n in (*names, "post_resolve")})
                                       for y, x in points])


def response_metrics(a, b, eye):
    mask = a["mask"] & b["mask"]
    if not mask.any():
        raise ValueError("lake response: no common pixels")

    def operands(s):
        uv = s["uv"][mask].astype("d")
        tint = s["tint"][mask].astype("d")
        air = s["air"][mask, :3].astype("d")
        color = s["color"][mask, :3].astype("d")
        gain = (
            (1 + uv[:, 2:3] * (tint[:, :3] - 1))
            * (1 - uv[:, 3:4])
            * (1 - tint[:, 3:4])
            * air
        )
        return s["output"][mask, :3].astype("d"), color, gain, uv[:, :2]

    oa, ca, ga, ua = operands(a)
    ob, cb, gb, ub = operands(b)
    delta = ob - oa
    pred = (cb - ca) * gb
    local = ub.copy()
    if b["packed"]:
        local[:, 0] = local[:, 0] * 2 - eye
    image = b["tex"]
    h, w = mask.shape
    halves = np.split(image, 2, axis=1) if b["packed"] else None
    modeled = sample_map(image, ub)
    eye_only = sample_map(halves[eye] if b["packed"] else image, local)

    def mae(x):
        return float(np.abs(x).mean())

    def common_taps(s, name):
        source = s["upstream"][name]
        if s["packed"]:
            source = np.split(source, 2, axis=1)[eye]
        return sample_map(source, local)

    taps = {name: common_taps(b, name) - common_taps(a, name) for name in a["upstream"]}
    post = taps["post_atmosphere"]
    resolve = taps["post_resolve"]

    def stage_terms(ix):
        return dict(
            gbuffer_tap_deltas={
                k: np.abs(taps[k][ix]).mean(axis=0).tolist()
                for k in (
                    "gbuffer0",
                    "gbuffer1",
                    "gbuffer2",
                    "gbuffer3",
                    "geometry_depth",
                )
            },
            weighted_resolve_mae=mae((gb * resolve)[ix]),
            upstream_texture_only=mae((delta - gb * post)[ix]),
            resolve_only=mae((delta - gb * resolve)[ix]),
            sampled_resolve_delta=mae(resolve[ix]),
            sampled_atmosphere_delta=mae(post[ix]),
            atmosphere_increment=mae((post - resolve)[ix]),
            unchanged_gbuffer=int(
                np.logical_and.reduce(
                    [
                        np.all(taps[k][ix] == 0, axis=1)
                        for k in (
                            "gbuffer0",
                            "gbuffer1",
                            "gbuffer2",
                            "gbuffer3",
                            "geometry_depth",
                        )
                    ]
                ).sum()
            ),
        )

    def group(ix):
        return dict(
            pixels=len(ix),
            output=mae(delta[ix]),
            using_isolated_color=mae((delta - pred)[ix]),
            gain=gb[ix].mean(0).tolist(),
            color=mae((cb - ca)[ix]),
            using_eye_mirror=mae((delta + (eye_only - modeled) * gb)[ix]),
            gain_delta=mae((gb - ga)[ix]),
            **stage_terms(ix),
        )

    order = np.argsort(-np.abs(delta).mean(1), kind="stable")
    n = (len(order) + 99) // 100
    return dict(
        common=len(order),
        all=group(order),
        top_percent=group(order[:n]),
        remainder=group(order[n:]),
        source_footprint=source_footprint(a, b, eye, local, gb, resolve),
        a_model_error=mae(sample_map(a["tex"], ua) - ca),
        b_model_error=mae(modeled - cb),
    )


def analyze(manifest, regions, measure=metrics):
    r = read_json(regions)
    cases = Manifest.from_wire(read_json(manifest)).cases
    if len(cases) != len(r["cases"]):
        raise ValueError("lake samples: case count mismatch")
    return [
        dict(case=case.name, **row)
        for case, localized in zip(cases, r["cases"], strict=True)
        for row in analyze_case(manifest, r, case, localized, measure)
    ]


def analyze_case(manifest, r, case, localized, measure):
    root = manifest.parent
    if (
        r["status"] not in ("measured", "inconclusive")
        or r["errors"]
        or Path(r["manifest"]).resolve() != manifest.resolve()
        or case.name != localized["name"]
    ):
        raise ValueError("lake samples: incomplete or mismatched regional evidence")
    shots = {}
    for group in case.modes:
        entries, errors = validated_samples(root, group.mode, group.samples)
        if errors:
            raise ValueError(errors)
        for spec, shot in entries:
            for eye in group.mode.eyes:
                shots[shot.capture_id, eye] = load(
                    root,
                    group.mode,
                    spec,
                    shot,
                    eye,
                    measure is lighting_metrics,
                    measure is response_metrics,
                )
    rows = []
    for row in localized["repeatability"] + localized["comparisons"]:
        eye = row["eye"]
        if row["stage"] != ("water_after_left", "water_after_right")[eye]:
            continue
        if row["status"] != "measured" or row["errors"]:
            if row["pairs"]:
                raise ValueError("lake samples: partial regional pair group")
            continue
        for pair in row["pairs"]:
            a, b = (pair[k]["capture_id"] for k in ("a", "b"))
            rows.append(
                dict(
                    comparison=row["name"],
                    eye=eye,
                    a=a,
                    b=b,
                    **measure(shots[a, eye], shots[b, eye], eye),
                )
            )
    return rows


def displacement_classes(a, b, eye, common):
    h, w = common.shape
    def sample(s):
        tex = s['depth_tex']; th, tw = tex.shape
        uv = s['depth'][common, :2].astype(float)
        ix, iy = np.clip(np.floor(uv * [tw, th]).astype(int), [0, 0], [tw-1, th-1]).T
        if s['depth_bounded']:
            ix = np.minimum(ix, w-1)
        y, x = np.nonzero(common)
        bx = x + (eye*w if s['packed'] else 0)
        matched = (tex[iy,ix] == s['depth'][common,3]) & (tex[y,bx] == s['depth'][common,2])
        return ix - (eye*w if s['packed'] else 0), iy, matched
    ax, ay, am = sample(a); bx, by, bm = sample(b)
    crossed = (ax != bx) | (ay != by)
    base_diff = a['depth'][common,2] != b['depth'][common,2]
    upstream = a['depth_tex'][ay,ax] != b['depth_tex'][ay,ax+w*eye]
    depth_diff = a['depth'][common,3] != b['depth'][common,3]
    color = np.abs(a['color'][common,:3]-b['color'][common,:3]).mean(axis=1)
    output = np.abs(a['output'][common,:3]-b['output'][common,:3]).mean(axis=1)
    shaded = np.abs(a['shaded'][common,:3]-b['shaded'][common,:3]).mean(axis=1)
    labels = crossed.astype(int) + 2*upstream + 4*base_diff + 8*depth_diff
    labels = np.where(am & bm, labels, -1)
    return [dict(point_model_matched=int(c)>=0, address_cross=bool(c&1), fixed_candidate_depth_diff=bool(c&2),
                 base_depth_diff=bool(c&4), sampled_candidate_depth_diff=bool(c&8), pixels=int((labels==c).sum()),
                 color_abs_sum=float(color[labels==c].sum(dtype=float)), output_abs_sum=float(output[labels==c].sum(dtype=float)),
                 shaded_abs_sum=float(shaded[labels==c].sum(dtype=float))) if c>=0 else
            dict(point_model_matched=False,pixels=int((labels==c).sum()),color_abs_sum=float(color[labels==c].sum(dtype=float)),
                 output_abs_sum=float(output[labels==c].sum(dtype=float)),shaded_abs_sum=float(shaded[labels==c].sum(dtype=float)))
            for c in np.unique(labels)]


def stable_response(manifest, regions, case_name):
    cases = Manifest.from_wire(read_json(manifest)).cases
    (case,) = [c for c in cases if c.name == case_name]
    report = read_json(regions)
    if Path(report['manifest']).resolve() != manifest.resolve() or report['errors']:
        raise ValueError('lake response: mismatched regional report')
    (regional,) = [c for c in report['cases'] if c['name'] == case.name]
    strict = read_json(manifest.parent / 'comparison/report.json')
    strict_case, = [c for c in strict['cases'] if c['name'] == case.name]
    results = []
    for eye, ref in enumerate((Mode.LEFT_ONLY, Mode.RIGHT_ONLY)):
        (group,) = [g for g in regional['comparisons'] if g['eye'] == eye and
                    g['stage'] == ('water_after_left', 'water_after_right')[eye] and
                    g['name'] == f'{ref} vs single_pass']
        strict_group, = [g for g in strict_case['comparisons'] if g['name'] == f'{ref} vs single_pass, eye {eye}']
        if strict_group['status'] not in ('pass','fail') or strict_group['errors']:
            results.append(dict(case=case.name, eye=eye, status='ineligible', errors=strict_group['errors']))
            continue
        if group['status'] != 'measured' or group['errors'] or not group['pairs']:
            raise ValueError(f'lake response: ineligible {ref} comparison')
        shots = {}
        for mode in case.modes:
            if mode.mode not in (ref, Mode.SINGLE_PASS):
                continue
            entries, errors = validated_samples(manifest.parent, mode.mode, mode.samples)
            if errors:
                raise ValueError(errors)
            for spec, shot in entries:
                fields = load(manifest.parent, mode.mode, spec, shot, eye, response=True)
                shots[shot.capture_id] = {k: fields[k] for k in
                    ('packed', 'depth_bounded', 'mask', 'output', 'uv', 'tint', 'color', 'air', 'shaded', 'tex', 'upstream')}
                if 'mesh' in fields:
                    shots[shot.capture_id].update({k:fields[k] for k in ('mesh','mixed','view_xy','strength')})
                if 'depth_tex' in fields:
                    shots[shot.capture_id].update({k:fields[k] for k in ('depth','surface','depth_tex')})
                del fields
        pairs = [(p['a']['capture_id'], p['b']['capture_id']) for p in group['pairs']]
        common = np.logical_and.reduce([s['mask'] for s in shots.values()])
        if not common.any():
            raise ValueError('lake response: no persistent common selected-draw pixels')
        band = common.copy()
        center = (common.shape[1] - 0.5) / (2 * common.shape[1])
        for s in shots.values():
            if s['packed']:
                u = s['uv'][..., 0]
                band &= (u >= center) & (u <= 0.5) if eye == 0 else False
        minimum = np.full(common.shape, np.inf)
        for a, b in pairs:
            delta = shots[b]['output'][..., :3] - shots[a]['output'][..., :3]
            minimum = np.minimum(minimum, np.abs(delta).mean(axis=2))
        y, x = np.unravel_index(np.argmax(np.where(common, minimum, -1)), common.shape)
        def point(s):
            uv = s['uv'][y, x].astype(float)
            tint = s['tint'][y, x].astype(float)
            air = s['air'][y, x, :3].astype(float)
            local = uv[:2].copy()
            if s['packed']:
                local[0] = 2 * local[0] - eye
            return dict(uv=uv, local=local, gain=(1 + uv[2]*(tint[:3]-1))*(1-uv[3])*(1-tint[3])*air,
                        color=s['color'][y,x,:3], output=s['output'][y,x,:3], shaded=s['shaded'][y,x,:3])
        rows = []
        for left, right in pairs:
            a, b = shots[left], shots[right]
            pa, pb = point(a), point(b)
            physical = np.split(b['tex'], 2, axis=1)[eye]
            def tap(tex, uv):
                return sample_map(tex, uv[None, :])[0]
            aa, ba = (tap(tex, pa['local']) for tex in (a['tex'], physical))
            bb = tap(physical, pb['local'])
            packed = tap(b['tex'], pb['uv'][:2])
            delta = pb['output'] - pa['output']
            color = pb['color'] - pa['color']
            band_metrics = {k: (float(np.abs(b[k][band, :3].astype(float) - a[k][band, :3]).mean())
                                if band.any() else None) for k in ('output', 'color', 'shaded')}
            def displacement_terms(s):
                packed = s['packed']; h,w = s['mask'].shape
                uv = s['depth'][y,x].astype(float); surf = s['surface'][y,x].astype(float)
                base = np.array([(x+0.5)/w, (y+0.5)/h]); candidate = uv[:2].copy()
                final = s['uv'][y,x,:2].astype(float)
                scale = np.array([2 if packed else 1, 1]); origin = np.array([eye if packed else 0, 0])
                candidate = candidate*scale-origin; final = final*scale-origin
                free = surf[:2]*scale; depth_factor = np.clip(surf[2]/16,0,1)
                tex = s['depth_tex']; th,tw=tex.shape
                ix,iy = np.clip(np.floor(uv[:2]*[tw,th]).astype(int),[0,0],[tw-1,th-1]);bx=x+(eye*w if packed else 0)
                if s['depth_bounded']:
                    ix = min(ix, w-1)
                attempted = candidate-base; accepted = final-base; length_sq = attempted@attempted
                return dict(free_local=free.tolist(), base_column=surf[2], view_surface_z=surf[3],
                    candidate_local=attempted.tolist(), candidate_model_error=(attempted-free*depth_factor).tolist(),
                    final_local=accepted.tolist(), effective_retention=float(accepted@attempted/length_sq) if length_sq>1e-14 else None,
                    depth_samples=uv[2:].tolist(), point_reads=[float(tex[y,bx]),float(tex[iy,ix])],
                    candidate_texel=[int(ix),int(iy)])
            terms = dict(displacement_classes=displacement_classes(a,b,eye,common), displacement_reference=displacement_terms(a), displacement_sps=displacement_terms(b)) if 'surface' in a and 'surface' in b else {}
            if 'mesh' in a and 'mesh' in b:
                def operands(s):
                    return (s['mesh'][y,x,:3].astype(float), s['mixed'][y,x,:3].astype(float),
                            float(s['mixed'][y,x,3]), s['view_xy'], s['strength'])
                def free(mesh, mixed, atten, view, strength):
                    n = 3*mesh+mixed
                    return (n[:2]/np.linalg.norm(n) @ view)*strength*atten*[-5,5]
                aa0,bb0 = operands(a),operands(b)
                values=[free(*(bb0[:i]+aa0[i:])) for i in range(6)]
                terms.update(normals=dict(mesh=[v[0].tolist() for v in (aa0,bb0)],mixed=[v[1].tolist() for v in (aa0,bb0)],
                    attenuation=[aa0[2],bb0[2]], base_column=[float(s['mesh'][y,x,3]) for s in (a,b)],
                    free_local_model=[values[i].tolist() for i in (0,-1)],
                    substitutions={k:(values[i+1]-values[i]).tolist() for i,k in enumerate(('mesh','mixed','attenuation','view','strength'))}))
            xy = np.clip(mirror(pa['local'])*[common.shape[1],common.shape[0]]-0.5,0,[common.shape[1]-1,common.shape[0]-1])
            ix,iy = np.floor(xy).astype(int)
            fx,fy = xy-[ix,iy]
            taps = []
            for tx,wx in ((ix,1-fx),(min(ix+1,common.shape[1]-1),fx)):
                for ty,wy in ((iy,1-fy),(min(iy+1,common.shape[0]-1),fy)):
                    if not wx*wy: continue
                    differences = {}
                    for key in a['upstream']:
                        va = a['upstream'][key][ty,tx].astype(float)
                        vb = b['upstream'][key][ty,tx + common.shape[1]*eye].astype(float)
                        differences[key] = (vb-va).tolist()
                    taps.append(dict(x=int(tx),y=int(ty),weight=float(wx*wy),differences=differences))
            rows.append(dict(**terms, fixed_uv_source_taps=taps, reference=left, sps=right, inner_edge_mae=band_metrics, output=delta.tolist(),
                shaded=(pb['shaded']-pa['shaded']).tolist(), color=color.tolist(),
                isolated_color_model_residual=(delta-color*pb['gain']).tolist(),
                local_uv_delta=(pb['local']-pa['local']).tolist(), fixed_uv_texture=(ba-aa).tolist(),
                address_on_sps_texture=(bb-ba).tolist(), packed_minus_physical=(packed-bb).tolist(),
                reference_model_error=(aa-pa['color']).tolist(), sps_model_error=(packed-pb['color']).tolist()))
        results.append(dict(case=case.name, eye=eye, x=int(x), y=int(y), common=int(common.sum()), inner_edge_pixels=int(band.sum()),
                            minimum_output_mae=float(minimum[y,x]), pairs=rows))
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path)
    parser.add_argument("regions", type=Path)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--residual", action="store_true")
    group.add_argument("--lighting", action="store_true")
    group.add_argument("--response", action="store_true")
    group.add_argument("--stable-response", metavar="CASE")
    args = parser.parse_args()
    print(
        json.dumps(
            stable_response(args.manifest, args.regions, args.stable_response) if args.stable_response else analyze(
                args.manifest,
                args.regions,
                response_metrics
                if args.response
                else lighting_metrics
                if args.lighting
                else residual
                if args.residual
                else metrics,
            ),
            indent=2,
            allow_nan=False,
        )
    )
