"""Great-circle navigation helpers for the dispatch planner."""

import math

EARTH_RADIUS_NM = 3440.065


def distance_nm(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    """Great-circle distance between two points, in nautical miles."""
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp = p2 - p1
    dl = math.radians(lon2 - lon1)
    a = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * EARTH_RADIUS_NM * math.asin(math.sqrt(a))


def initial_course(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    """True course from the first point to the second, in degrees."""
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dl = math.radians(lon2 - lon1)
    x = math.sin(dl) * math.cos(p2)
    y = math.cos(p1) * math.sin(p2) - math.sin(p1) * math.cos(p2) * math.cos(dl)
    return (math.degrees(math.atan2(x, y)) + 360) % 360


def ground_speed(tas_kt: float, wind_dir: float, wind_kt: float, course: float) -> float:
    """Ground speed along the course, for a wind blowing from wind_dir."""
    angle = math.radians(wind_dir - course)
    headwind = wind_kt * math.cos(angle)
    crosswind = wind_kt * math.sin(angle)
    return math.sqrt(max(tas_kt**2 - crosswind**2, 0.0)) - headwind
