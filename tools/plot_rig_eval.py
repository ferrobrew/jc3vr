# /// script
# requires-python = ">=3.11"
# dependencies = ["matplotlib", "numpy"]
# ///
"""Plot a `tracking-synth rig-eval` CSV against the take's video time.

Usage: plot_rig_eval.py <rig-eval.csv> <out dir> [<from>-<to> ...]

Writes `overview.png` for the whole take, and one `window_<from>_<to>.png` per stretch of video
seconds given. Each plot shows the neck's offset over the pelvis against the lean's reach and the
step demand, with the step phases shaded; the bend; the neck's height against the standing height
and the crouch; the head's and the body's yaw with the turns shaded; and the yaw error, the
simulated pelvis's speed, the steps committed, and the stick.
"""
import csv, sys, os
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

path = sys.argv[1]; out = sys.argv[2]; os.makedirs(out, exist_ok=True)
rows = list(csv.DictReader(open(path)))
def col(name, f=float):
    return np.array([f(r[name]) if r[name] != "" else np.nan for r in rows])
v = col("video_s"); t = np.where(np.isnan(v), np.nan, v)
phase = col("phase"); steps = col("steps"); demand = col("demand_m"); off = col("offset_m"); reach = col("lean_reach_m")
bend = col("bend_deg"); pitch = col("pitch_deg"); crouch = col("crouch_m"); neck_y = col("neck_y_m"); stand = col("standing_neck_m")
hy = col("head_yaw_deg"); by = col("body_yaw_deg"); ye = col("yaw_error_deg"); turning = col("turning"); speed = col("speed_mps"); moving = col("moving")

def plot(lo, hi, name, title):
    m = (t >= lo) & (t <= hi)
    if m.sum() < 2: return
    fig, ax = plt.subplots(5, 1, figsize=(14, 13), sharex=True)
    tt = t[m]
    ax[0].plot(tt, off[m], label="neck offset over pelvis (m)", color="C0")
    ax[0].plot(tt, reach[m], label="lean reach (m)", color="C2", ls="--")
    ax[0].plot(tt, demand[m], label="step demand (m)", color="C3")
    ax[0].fill_between(tt, 0, np.where(phase[m] == 1, 0.6, 0), color="C3", alpha=0.12, step="mid", label="stepping")
    ax[0].fill_between(tt, 0, np.where(phase[m] == 2, 0.6, 0), color="C1", alpha=0.12, step="mid", label="settling")
    ax[0].set_ylim(0, 0.6); ax[0].legend(loc="upper right", fontsize=8); ax[0].set_ylabel("m")
    ax[1].plot(tt, bend[m], label="bend (deg)"); ax[1].plot(tt, pitch[m], label="pitch (deg)", alpha=0.7)
    ax[1].legend(loc="upper right", fontsize=8); ax[1].set_ylabel("deg")
    ax[2].plot(tt, neck_y[m], label="neck height (m)"); ax[2].plot(tt, stand[m], label="standing neck (m)", ls="--")
    ax[2].plot(tt, 1.0 + crouch[m], label="1 + crouch (m)", alpha=0.7)
    ax[2].legend(loc="lower right", fontsize=8)
    hyu = np.degrees(np.unwrap(np.radians(hy[m]))); byu = np.degrees(np.unwrap(np.radians(by[m])))
    byu += 360 * np.round((hyu[0] - byu[0]) / 360)
    ax[3].plot(tt, hyu, label="head yaw (deg)"); ax[3].plot(tt, byu, label="body yaw (deg)")
    ax[3].fill_between(tt, np.nanmin(hyu), np.where(turning[m] > 0, np.nanmax(hyu), np.nanmin(hyu)), color="C4", alpha=0.12, step="mid", label="turning")
    ax[3].legend(loc="upper right", fontsize=8)
    ax[4].plot(tt, ye[m], label="yaw error head-body (deg)")
    ax[4].plot(tt, speed[m] * 50, label="pelvis speed x50 (m/s)")
    ax[4].plot(tt, steps[m] - steps[m][0], label="steps since window start")
    ax[4].fill_between(tt, -60, np.where(moving[m] > 0, 60, -60), color="grey", alpha=0.1, step="mid", label="stick")
    ax[4].set_ylim(-60, 60); ax[4].legend(loc="upper right", fontsize=8); ax[4].set_xlabel("video time (s)")
    fig.suptitle(title); fig.tight_layout(); fig.savefig(os.path.join(out, name), dpi=80); plt.close(fig)

plot(np.nanmin(t), np.nanmax(t), "overview.png", "whole session")
for window in sys.argv[3:]:
    lo, hi = (float(x) for x in window.split("-"))
    plot(lo, hi, f"window_{lo:g}_{hi:g}.png", f"video {lo:g}-{hi:g} s")
print("ok", len(rows))
