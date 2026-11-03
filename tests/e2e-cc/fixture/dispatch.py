"""Plans a route leg by leg and prints the dispatch sheet."""

import json
import sys
from pathlib import Path

from navigation import distance_nm, ground_speed, initial_course


def plan(route: dict) -> list[dict]:
    legs = []
    points = route["points"]
    for a, b in zip(points, points[1:], strict=False):
        dist = distance_nm(a["lat"], a["lon"], b["lat"], b["lon"])
        course = initial_course(a["lat"], a["lon"], b["lat"], b["lon"])
        gs = ground_speed(route["tas_kt"], route["wind_dir"], route["wind_kt"], course)
        legs.append({"from": a["id"], "to": b["id"], "nm": round(dist, 1), "hours": round(dist / gs, 2)})
    return legs


def main() -> None:
    route = json.loads(Path(sys.argv[1] if len(sys.argv) > 1 else "data/route.json").read_text())
    for leg in plan(route):
        print(f"{leg['from']} -> {leg['to']}: {leg['nm']} nm, {leg['hours']} h")


if __name__ == "__main__":
    main()
