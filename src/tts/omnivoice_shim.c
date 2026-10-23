// Flat wrappers over omnivoice.h so Rust declares no struct layouts. Built by build.rs with the
// `omnivoice` feature on Windows.
#include <stdlib.h>
#include <string.h>

#include "omnivoice.h"

struct hm_ref {
    struct ov_voice_ref codes;
    char *              text;
};

extern void hammurabi_ov_log(int level, const char * msg);

static void log_cb(enum ov_log_level level, const char * msg, void * user_data) {
    (void) user_data;
    hammurabi_ov_log((int) level, msg);
}

const char * hm_ov_error(void) {
    return ov_last_error();
}

struct ov_context * hm_ov_init(const char * model, const char * codec, const char * device) {
    struct ov_init_params p;
    ov_log_set(log_cb, NULL);
    ov_init_default_params(&p);
    p.model_path = model;
    p.codec_path = codec;
    p.device     = device;
    return ov_init(&p);
}

struct hm_ref * hm_ov_extract_ref(struct ov_context * ctx, const float * pcm24k, int n, const char * text) {
    struct hm_ref * r = calloc(1, sizeof *r);
    if (!r) {
        return NULL;
    }
    if (ov_extract_voice_ref(ctx, pcm24k, n, &r->codes) != OV_STATUS_OK) {
        free(r);
        return NULL;
    }
    r->text = malloc(strlen(text) + 1);
    if (r->text) {
        strcpy(r->text, text);
    }
    return r;
}

// Reference length in codec frames (25 per second).
int hm_ov_ref_frames(const struct hm_ref * r) {
    return r->codes.ref_T;
}

void hm_ov_free_ref(struct hm_ref * r) {
    if (r) {
        ov_voice_ref_free(&r->codes);
        free(r->text);
        free(r);
    }
}

// ref NULL = OmniVoice's own voice; seconds > 0 forces the output length. Returns an ov_status;
// on success *out holds *n samples of 24 kHz mono audio, released with hm_ov_free_audio.
int hm_ov_synth(struct ov_context * ctx, const struct hm_ref * ref, const char * text, const char * lang, int steps,
                unsigned long long seed, float seconds, float ** out, int * n) {
    struct ov_tts_params p;
    struct ov_audio      a;
    enum ov_status       rc;
    ov_tts_default_params(&p);
    p.text        = text;
    p.lang        = lang;
    p.mg_num_step = steps;
    p.mg_seed     = seed;
    if (seconds > 0.0f) {
        p.T_override = ov_duration_sec_to_tokens(ctx, seconds);
    }
    if (ref) {
        p.ref_audio_tokens = ref->codes.ref_codes;
        p.ref_T            = ref->codes.ref_T;
        p.ref_text         = ref->text;
    }
    memset(&a, 0, sizeof a);
    rc = ov_synthesize(ctx, &p, &a);
    *out = a.samples;
    *n   = a.n_samples;
    return (int) rc;
}

void hm_ov_free_audio(float * samples, int n) {
    struct ov_audio a;
    a.samples     = samples;
    a.n_samples   = n;
    a.sample_rate = 24000;
    a.channels    = 1;
    ov_audio_free(&a);
}

void hm_ov_free(struct ov_context * ctx) {
    ov_free(ctx);
}
