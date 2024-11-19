#version 450
#define NO_TEXTURE 4294967295
#define WORKFLOW_MAIN 0
#define WORKFLOW_TEXT 1

layout(location = 0) in vec4 in_color;
layout(location = 1) in vec2 in_uv;
layout(location = 0) out vec4 out_color;
layout(constant_id = 0) const int WORKFLOW = 0;

layout(set = 0, binding = 0) uniform sampler2D textures[1000];
layout(push_constant) uniform push_constants {
    uint texture_id;
};

void main() {
    if (texture_id == NO_TEXTURE) {
        out_color = in_color;
        return;
    }

    if (WORKFLOW == WORKFLOW_TEXT) {
        float coverage = texture(textures[texture_id], in_uv).r;
        out_color = in_color * coverage;
    } else {
        vec4 user_texture = texture(textures[texture_id], in_uv);
        out_color = in_color * user_texture;
    }
}
