from __future__ import annotations

from .base import DisplayProfile
from .xteink_x4_portrait import PROFILE as XTEINK_X4_PORTRAIT

_PROFILES_BY_NAME: dict[str, DisplayProfile] = {
    XTEINK_X4_PORTRAIT.name: XTEINK_X4_PORTRAIT,
}


def get_profile(name: str) -> DisplayProfile:
    profile = _PROFILES_BY_NAME.get(name)
    if profile is None:
        raise ValueError(f"unsupported profile: {name}")
    return profile


__all__ = ["DisplayProfile", "XTEINK_X4_PORTRAIT", "get_profile"]
