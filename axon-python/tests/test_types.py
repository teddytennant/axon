"""Unit tests for axon.types protocol primitives."""

from axon.types import Capability, TaskRequest, TaskResponse, TaskStatus


def test_capability_tag():
    assert Capability("llm", "chat", 1).tag() == "llm:chat:v1"


def test_capability_matches_exact():
    cap = Capability("llm", "chat", 1)
    assert cap.matches(Capability("llm", "chat", 1))


def test_capability_matches_higher_version_provider():
    # A provider at v2 satisfies a request for v1.
    provider = Capability("llm", "chat", 2)
    assert provider.matches(Capability("llm", "chat", 1))


def test_capability_lower_version_provider_fails():
    provider = Capability("llm", "chat", 1)
    assert not provider.matches(Capability("llm", "chat", 2))


def test_capability_different_name_fails():
    provider = Capability("llm", "chat", 1)
    assert not provider.matches(Capability("llm", "complete", 1))


def test_capability_different_namespace_fails():
    provider = Capability("llm", "chat", 1)
    assert not provider.matches(Capability("text", "chat", 1))


def test_capability_parse_tag_roundtrip():
    cap = Capability("llm", "chat", 3)
    assert Capability.parse_tag(cap.tag()) == cap


def test_capability_json_roundtrip():
    cap = Capability("json", "upper", 2)
    assert Capability.from_json(cap.to_json()) == cap


def test_capability_is_hashable():
    s = {Capability("a", "b", 1), Capability("a", "b", 1)}
    assert len(s) == 1


def test_task_request_json_roundtrip():
    req = TaskRequest(Capability("echo", "ping", 1), b"hello", timeout_ms=1234)
    back = TaskRequest.from_json(req.to_json())
    assert back.id == req.id
    assert back.capability == req.capability
    assert back.payload == b"hello"
    assert back.timeout_ms == 1234


def test_task_request_payload_base64_non_utf8():
    req = TaskRequest(Capability("echo", "ping", 1), b"\xff\x00")
    data = req.to_json()
    assert isinstance(data["payload"], str)
    assert TaskRequest.from_json(data).payload == b"\xff\x00"


def test_task_response_json_roundtrip():
    resp = TaskResponse("req-1", TaskStatus.success(), b"\xff\x00\x10", duration_ms=42)
    back = TaskResponse.from_json(resp.to_json())
    assert back.request_id == "req-1"
    assert back.status == TaskStatus.success()
    assert back.payload == b"\xff\x00\x10"
    assert back.duration_ms == 42


def test_task_status_success_json():
    assert TaskStatus.success().to_json() == "Success"


def test_task_status_error_json():
    assert TaskStatus.error("boom").to_json() == {"Error": "boom"}


def test_task_status_from_json_unit():
    assert TaskStatus.from_json("Timeout") == TaskStatus.timeout()
    assert TaskStatus.from_json("NoCapability") == TaskStatus.no_capability()


def test_task_status_from_json_error():
    status = TaskStatus.from_json({"Error": "nope"})
    assert status == TaskStatus.error("nope")
    assert status.kind == "Error"
    assert status.message == "nope"


def test_task_status_equality_and_is_success():
    assert TaskStatus.success().is_success
    assert not TaskStatus.error("x").is_success
    assert TaskStatus.success() != TaskStatus.error("x")
    assert TaskStatus.error("a") != TaskStatus.error("b")
