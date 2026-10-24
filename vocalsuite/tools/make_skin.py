#!/usr/bin/env python3
"""Crea la "pelle" dell'interfaccia di SR Vocal Suite dall'immagine di riferimento.

L'interfaccia e' l'immagine di riferimento (1448 x 1086) senza le parti che
cambiano mentre il plugin lavora: quelle le disegna il plugin sopra, nello
stesso punto e con lo stesso stile.

Tolte dall'immagine (e ridisegnate dal vivo):
- i valori dei menu in alto (Alto/Tenor, A Major, Default*, 440 Hz, Celestial Clean);
- la tonalita' nella sfera ("A" e "MAJOR");
- i numeri delle manopole, i pallini e l'arco luminoso del valore;
- i pallini di Retune Speed e Humanize;
- la curva dell'intonazione e il nome della nota ("C4");
- i meter di Input e Output;
- i pulsanti Bypass e Monitor (cambiano aspetto quando sono accesi).

Uso:  python3 vocalsuite/tools/make_skin.py riferimento.jpg vocalsuite/assets/skin.png
Servono numpy e opencv-python.
"""

import math
import sys

import cv2
import numpy as np

# Le misure (pixel dell'immagine di riferimento) sono le stesse di
# vocalsuite/src/layout.rs: se cambi qualcosa qui, cambialo anche li'.
KNOBS = {  # nome: (centro x, centro y, raggio dell'arco, raggio del pallino, pallino attuale x, y)
    "chorus": (192.5, 872.5, 103.0, 82.0, 126.5, 825.0),
    "formant": (552.5, 872.5, 108.5, 82.0, 595.0, 800.0),
    "reverb": (907.5, 874.5, 104.0, 82.0, 901.0, 791.5),
    "delay": (1265.5, 873.5, 102.5, 82.0, 1187.0, 846.0),
}
KNOB_TEXT = {"chorus": (176, 862, 212, 891), "formant": (529, 862, 566, 891), "reverb": (887, 862, 925, 891), "delay": (1249, 862, 1284, 891)}
SMALL_DOTS = [(1036.0, 190.0), (1178.5, 314.0)]
SMALL_CENTERS = [(1000.5, 229.5), (1143.5, 358.5)]
HEADER_TEXT = [(447, 56, 533, 79), (654, 56, 721, 79), (905, 32, 977, 52), (985, 35, 1039, 51), (910, 56, 1007, 72)]
ORB_TEXT = [(655, 245, 762, 350), (662, 356, 751, 377)]
GRAPH = (128, 483, 1414, 597)  # interno del grafico (senza le etichette a sinistra)
NOTE_LABEL = (48, 532, 78, 554)
METERS = [(119, 636, 336, 668), (1249, 636, 1405, 668)]
PILLS = (520, 623, 852, 681)


def rotate_clone(img, cx, cy, dx, dy, radius, degrees):
    """Copia sul pallino (dx, dy) il contenuto ruotato di `degrees` intorno
    al centro della manopola, con il bordo sfumato."""
    a = math.radians(degrees)
    ca, sa = math.cos(a), math.sin(a)
    src = img.copy()
    for y in range(int(dy - radius - 1), int(dy + radius + 2)):
        for x in range(int(dx - radius - 1), int(dx + radius + 2)):
            d = math.hypot(x - dx, y - dy)
            if d > radius:
                continue
            # punto sorgente: ruotato intorno al centro
            rx, ry = x - cx, y - cy
            sx = cx + rx * ca - ry * sa
            sy = cy + rx * sa + ry * ca
            k = min(1.0, (radius - d) / 5.0)
            img[y, x] = img[y, x] * (1 - k) + cv2.getRectSubPix(src, (1, 1), (sx, sy))[0, 0] * k


def coons_fill(img, box, feather=0):
    """Riempie il rettangolo con una superficie liscia che combacia con i
    quattro bordi (patch di Coons), sfumata di `feather` pixel."""
    x0, y0, x1, y1 = box
    smooth = lambda v: cv2.GaussianBlur(v.reshape(1, -1, 3), (0, 0), 5.0).reshape(-1, 3) if len(v) > 12 else v
    top = smooth(img[y0, x0:x1 + 1].copy())
    bot = smooth(img[y1, x0:x1 + 1].copy())
    left = smooth(img[y0:y1 + 1, x0].copy())
    right = smooth(img[y0:y1 + 1, x1].copy())
    c00, c10, c01, c11 = img[y0, x0].copy(), img[y0, x1].copy(), img[y1, x0].copy(), img[y1, x1].copy()
    src = img.copy()
    for j, y in enumerate(range(y0, y1 + 1)):
        v = j / (y1 - y0)
        for i, x in enumerate(range(x0, x1 + 1)):
            u = i / (x1 - x0)
            val = (1 - v) * top[i] + v * bot[i] + (1 - u) * left[j] + u * right[j] \
                - ((1 - u) * (1 - v) * c00 + u * (1 - v) * c10 + (1 - u) * v * c01 + u * v * c11)
            if feather > 0:
                edge = min(i, j, x1 - x0 - i, y1 - y0 - j)
                k = min(1.0, edge / feather)
                val = src[y, x] * (1 - k) + val * k
            img[y, x] = val


def text_mask(gray, box, thr, grow):
    x0, y0, x1, y1 = box
    m = np.zeros(gray.shape, np.uint8)
    sub = (gray[y0:y1, x0:x1] > thr).astype(np.uint8) * 255
    m[y0:y1, x0:x1] = sub
    return cv2.dilate(m, np.ones((grow, grow), np.uint8))


def polar_patch(img, cx, cy, r0, r1, a_from, a_to, profile):
    """Riscrive la corona r0..r1 tra gli angoli (gradi, 0 = in alto, orario)
    con il profilo radiale `profile` (colore per raggio), sfumando i bordi."""
    h, w = img.shape[:2]
    ys, xs = np.mgrid[int(cy - r1 - 2):int(cy + r1 + 3), int(cx - r1 - 2):int(cx + r1 + 3)]
    dx, dy = xs - cx, ys - cy
    r = np.hypot(dx, dy)
    ang = np.degrees(np.arctan2(dx, -dy))
    sel = (r >= r0) & (r <= r1) & (ang >= a_from) & (ang <= a_to)
    for y, x, rr, aa in zip(ys[sel], xs[sel], r[sel], ang[sel]):
        if not (0 <= x < w and 0 <= y < h):
            continue
        k = min((rr - r0) / 2.0, (r1 - rr) / 2.0, (aa - a_from) / 4.0, (a_to - aa) / 4.0, 1.0)
        k = max(0.0, k)
        i = int(round((rr - r0) / (r1 - r0) * (len(profile) - 1)))
        img[y, x] = img[y, x] * (1 - k) + profile[i] * k


def radial_profile(img, cx, cy, r0, r1, a_from, a_to, steps=40):
    out = []
    for i in range(steps):
        rr = r0 + (r1 - r0) * i / (steps - 1)
        vals = []
        for a in np.linspace(a_from, a_to, 90):
            x = cx + rr * math.sin(math.radians(a))
            y = cy - rr * math.cos(math.radians(a))
            vals.append(img[int(round(y)), int(round(x))])
        out.append(np.median(np.array(vals), axis=0))
    return np.array(out)


def main(src, dst):
    img = cv2.imread(src)
    gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
    mask = np.zeros(gray.shape, np.uint8)

    for box in HEADER_TEXT:
        mask |= text_mask(gray, box, 60, 5)
    # (la "A" e "MAJOR" della sfera si tolgono dopo, con una toppa sfumata)
    for box in KNOB_TEXT.values():
        mask |= text_mask(gray, box, 100, 5)
    x0, y0, x1, y1 = NOTE_LABEL
    mask |= text_mask(gray, NOTE_LABEL, 50, 5)

    out = cv2.inpaint(img, mask, 6, cv2.INPAINT_TELEA).astype(np.float32)

    # Pallini: al loro posto lo stesso anello girato di qualche grado
    # (l'anello continua uguale, senza macchie).
    for (cx, cy, _, _, dx, dy) in KNOBS.values():
        rotate_clone(out, cx, cy, dx, dy, 18, 24.0)
    for (dx, dy), (cx, cy) in zip(SMALL_DOTS, SMALL_CENTERS):
        rotate_clone(out, cx, cy, dx, dy, 17, -34.0)

    # Meter e pulsanti: superficie liscia raccordata ai quattro bordi.
    for box in METERS:
        coons_fill(out, box)
    coons_fill(out, PILLS)
    # La tonalita' nella sfera.
    for box in ORB_TEXT:
        coons_fill(out, box, feather=6)

    # Archi del valore delle manopole: la parte accesa (da 0 al pallino)
    # diventa come la parte spenta (dal pallino a 100).
    for name, (cx, cy, ra, rd, dx, dy) in KNOBS.items():
        dot_angle = math.degrees(math.atan2(dx - cx, -(dy - cy)))
        dim = radial_profile(out, cx, cy, ra - 7, ra + 10, dot_angle + 25, 140)
        polar_patch(out, cx, cy, ra - 7, ra + 10, -158, dot_angle + 18, dim)

    # Grafico: sfondo ricostruito colonna per colonna dalle righe in alto e
    # in basso (restano le linee verticali della griglia), senza curva.
    x0, y0, x1, y1 = GRAPH
    top = np.median(out[y0 + 3:y0 + 15, x0:x1], axis=0)
    bot = np.median(out[y1 - 13:y1 - 1, x0:x1], axis=0)
    top = cv2.blur(top.reshape(1, -1, 3), (3, 1)).reshape(-1, 3)
    bot = cv2.blur(bot.reshape(1, -1, 3), (3, 1)).reshape(-1, 3)
    for y in range(y0 + 2, y1):
        t = (y - y0) / (y1 - y0)
        out[y, x0:x1] = top * (1 - t) + bot * t
    # Raccordo con le etichette a sinistra.
    for x in range(x0, x0 + 6):
        k = (x - x0) / 6.0
        out[y0 + 2:y1, x] = out[y0 + 2:y1, x0 - 1] * (1 - k) + out[y0 + 2:y1, x] * k

    cv2.imwrite(dst, np.clip(out, 0, 255).astype(np.uint8))
    print("pelle salvata in", dst)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
