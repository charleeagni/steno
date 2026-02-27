import torch
import torch.nn as nn
import os

def init_jit_model(model_path: str):
    torch.set_grad_enabled(False)
    model = torch.jit.load(model_path)
    model.eval()
    return model

def convert_to_onnx(model_path: str, output_path: str):
    # Load the model
    model = init_jit_model(model_path)
    
    # Create dummy input
    dummy_input = torch.randn(1, 512)
    dummy_state = torch.zeros(2, 1, 128)
    dummy_sr = torch.tensor([16000], dtype=torch.int64)
    
    # Export the model
    torch.onnx.export(
        model,
        (dummy_input, dummy_state, dummy_sr),
        output_path,
        export_params=True,
        opset_version=16,
        do_constant_folding=True,
        input_names=['input', 'state', 'sr'],
        output_names=['output', 'new_state'],
        dynamic_axes={
            'input': {0: 'batch_size', 1: 'sequence'},
            'state': {1: 'batch_size'},
            'output': {0: 'batch_size'},
            'new_state': {1: 'batch_size'}
        }
    )
    print(f"Model converted and saved to {output_path}")

if __name__ == "__main__":
    # Create models directory if it doesn't exist
    os.makedirs("models", exist_ok=True)
    
    # Download JIT model
    import urllib.request
    jit_model_url = "https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.jit"
    jit_model_path = "models/silero_vad.jit"
    
    print("Downloading JIT model...")
    urllib.request.urlretrieve(jit_model_url, jit_model_path)
    
    # Convert to ONNX
    print("Converting to ONNX...")
    convert_to_onnx(jit_model_path, "models/silero_vad.onnx") 