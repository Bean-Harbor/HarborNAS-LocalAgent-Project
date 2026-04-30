import importlib.machinery
import importlib.util
import sys

from conftest import ROOT


def load_sidecar_module():
    path = ROOT / "tools/release_templates/bin/harbor-vlm-sidecar"
    loader = importlib.machinery.SourceFileLoader("harbor_vlm_sidecar", str(path))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[loader.name] = module
    loader.exec_module(module)
    return module


def test_sidecar_extracts_openai_compatible_prompt_and_image_url() -> None:
    module = load_sidecar_module()
    payload = {
        "model": "HuggingFaceTB/SmolVLM-256M-Instruct",
        "messages": [
            {
                "role": "system",
                "content": "Camera alarm prompt that should not steer photo indexing.",
            },
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe the photo content."},
                    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,YWJj"}},
                ],
            }
        ],
    }

    prompt, image_url = module.extract_prompt_and_image_url(payload)

    assert prompt == "Describe the photo content."
    assert image_url == "data:image/jpeg;base64,YWJj"


def test_sidecar_rejects_non_data_url_images() -> None:
    module = load_sidecar_module()

    try:
        module.decode_data_url("https://example.invalid/photo.jpg")
    except ValueError as exc:
        assert "data:image" in str(exc)
    else:  # pragma: no cover - assertion guard
        raise AssertionError("expected decode_data_url to reject remote URLs")


def test_sidecar_openai_response_shape_preserves_model_and_text() -> None:
    module = load_sidecar_module()

    response = module.openai_chat_response("HuggingFaceTB/SmolVLM-256M-Instruct", "photo caption")

    assert response["object"] == "chat.completion"
    assert response["model"] == "HuggingFaceTB/SmolVLM-256M-Instruct"
    assert response["choices"][0]["message"]["content"] == "photo caption"
