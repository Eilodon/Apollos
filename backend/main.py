from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from google.adk.runners import Runner
from google.adk.sessions import InMemorySessionService
from .agent.aria_agent import create_aria_agent
from .agent.run_config import get_run_config
import asyncio

app = FastAPI(title="VisionGPT ARIA - Pure ADK Backend")

# Khởi tạo Pure ADK
root_agent = create_aria_agent()
session_service = InMemorySessionService()
runner = Runner(
    agent=root_agent,
    session_service=session_service,
    app_name="visiongpt-aria"
)

run_config = get_run_config()

# Lưu active WS cho HARD_STOP emit
active_connections: dict[str, WebSocket] = {}

@app.websocket("/ws/live/{session_id}")
async def websocket_live_endpoint(websocket: WebSocket, session_id: str):
    await websocket.accept()
    active_connections[session_id] = websocket
    
    try:
        # Pure ADK run_live (xử lý video frame + audio chunk từ client)
        async for event in runner.run_live(
            session_id=session_id,
            run_config=run_config
        ):
            if event.type == "audio":
                await websocket.send_bytes(event.audio_data)          # Gửi voice về PWA
            elif event.type == "function_call":
                await handle_function_call(event, websocket, session_id)
                
    except WebSocketDisconnect:
        print(f"Session {session_id} disconnected")
    finally:
        active_connections.pop(session_id, None)

async def handle_function_call(event, websocket: WebSocket, session_id: str):
    """Xử lý HARD_STOP khi Gemini gọi tool"""
    if event.tool_name == "log_hazard_event":
        payload = {
            "type": "HARD_STOP",
            "position_x": event.args.get("position_x", 0.0),
            "distance": event.args.get("distance_category", "mid"),
            "hazard_type": event.args.get("hazard_type")
        }
        await websocket.send_json(payload)   # Trigger siren ngay lập tức
