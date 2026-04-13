"""weather.query local builtin handler."""
from __future__ import annotations

import json
import os
import urllib.parse
import urllib.request
from typing import Any


def handle(
    operation: str,
    city: str = "",
    date: str = "today",
    units: str = "metric",
    language: str = "zh-CN",
    include_source: bool = True,
    **_: Any,
) -> dict[str, Any]:
    if operation != "query":
        return {"error": f"Unknown operation: {operation!r}"}

    resolved_city = (
        city
        or os.environ.get("HARBOR_WEATHER_DEFAULT_CITY")
        or os.environ.get("HARBOR_DEFAULT_CITY")
        or ""
    ).strip()
    if not resolved_city:
        return {"error": "weather.query requires a city or HARBOR_WEATHER_DEFAULT_CITY"}

    geo = _geocode_city(resolved_city, language)
    forecast = _fetch_weather(geo["latitude"], geo["longitude"], units)

    result = {
        "city": geo["name"],
        "country": geo.get("country", ""),
        "date": date,
        "temperature": forecast["temperature"],
        "wind_speed": forecast.get("wind_speed"),
        "summary": _weather_summary(forecast.get("weather_code"), language),
        "observed_at": forecast.get("time", ""),
        "units": units,
    }
    if include_source:
        result["source"] = "open-meteo"
    return result


def _geocode_city(city: str, language: str) -> dict[str, Any]:
    params = urllib.parse.urlencode({"name": city, "count": 1, "language": language})
    with urllib.request.urlopen(f"https://geocoding-api.open-meteo.com/v1/search?{params}", timeout=10) as response:
        data = json.loads(response.read().decode("utf-8"))

    results = data.get("results") or []
    if not results:
        raise RuntimeError(f"City not found: {city}")
    return results[0]


def _fetch_weather(latitude: float, longitude: float, units: str) -> dict[str, Any]:
    params = {
        "latitude": latitude,
        "longitude": longitude,
        "current": "temperature_2m,wind_speed_10m,weather_code",
    }
    if units == "imperial":
        params["temperature_unit"] = "fahrenheit"
        params["wind_speed_unit"] = "mph"
    query = urllib.parse.urlencode(params)
    with urllib.request.urlopen(f"https://api.open-meteo.com/v1/forecast?{query}", timeout=10) as response:
        data = json.loads(response.read().decode("utf-8"))

    current = data.get("current") or {}
    return {
        "temperature": current.get("temperature_2m"),
        "wind_speed": current.get("wind_speed_10m"),
        "weather_code": current.get("weather_code"),
        "time": current.get("time", ""),
    }


def _weather_summary(code: int | None, language: str) -> str:
    summaries = {
        0: "晴朗" if language.startswith("zh") else "Clear",
        1: "大致晴朗" if language.startswith("zh") else "Mainly clear",
        2: "局部多云" if language.startswith("zh") else "Partly cloudy",
        3: "阴" if language.startswith("zh") else "Overcast",
        45: "雾" if language.startswith("zh") else "Fog",
        61: "小雨" if language.startswith("zh") else "Rain",
        63: "中雨" if language.startswith("zh") else "Moderate rain",
        65: "大雨" if language.startswith("zh") else "Heavy rain",
        71: "小雪" if language.startswith("zh") else "Snow",
        95: "雷暴" if language.startswith("zh") else "Thunderstorm",
    }
    return summaries.get(code, "天气未知" if language.startswith("zh") else "Unknown weather")
