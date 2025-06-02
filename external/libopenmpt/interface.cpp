/************************************************************************/
/* MODinfo 2.0 by Saga Musix, 2015 - 2016                               */
/* BSD 3-clause                                                         */
/* uses libopenmpt when possible, custom code otherwise                 */
/* compile with g++ modinfo.cpp -Wall -std=c++17 -lopenmpt -o modinfo    */
/************************************************************************/

#include <exception>
#include <fstream>
#include <sstream>
#include <iostream>
#include <stdexcept>
#include <vector>
#include <cstdint>
#include <cstring>
#include <libopenmpt/libopenmpt.hpp>
#include <libopenmpt/libopenmpt_ext.hpp>
#include <stdafx.h>
#include <soundlib/Sndfile.h>
#include <stdint.h>
//#include "../../DebugHeap.h"

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

//DebugHeap* g_debug_heap = nullptr;

#define DebugHeapAllocate(heap, size, align) malloc(size)
#define DebugHeapFree(heap, ptr) free(ptr)

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

static uint64_t hash_patterns(openmpt::module& mod, int dump_patterns) {
    // Hash pattern data using 64-bit FNV-1a
    uint64_t hash = 14695981039346656037ull;

    //dump_patterns = 1;

    const int32_t num_channels = mod.get_num_channels();
    const int32_t num_songs = mod.get_num_subsongs();
    for (auto s = 0; s < num_songs; s++) {
        mod.select_subsong(s);
        if (mod.get_current_order() != 0) {
            // Ignore hidden subsongs, as we go through the whole oder list anyway.
            continue;
        }

        const auto num_orders = mod.get_num_orders();
        // Go through the complete sequence order by order.
        for (auto o = 0; o < num_orders; o++) {
            const int32_t p = mod.get_order_pattern(o);
            const int32_t num_rows = mod.get_pattern_num_rows(p);
            if (dump_patterns)
                printf("=======================================================\n");

            for (auto r = 0; r < num_rows; r++) {
                for (auto c = 0; c < num_channels; c++) {
                    const uint8_t note = mod.get_pattern_row_channel_command(p, r, c, openmpt::module::command_note);
                    const uint8_t effect = mod.get_pattern_row_channel_command(p, r, c, openmpt::module::command_effect);
                    const uint8_t parameter = mod.get_pattern_row_channel_command(p, r, c, openmpt::module::command_parameter);

                    if (effect == 1 && parameter == 0xff) {
                        return 1;
                    }

                    if (note != 0) {
                        hash ^= note;
                        hash *= 1099511628211ull;
                    }

                    if (dump_patterns) {
                        std::string t = mod.format_pattern_row_channel_command(p, r, c, openmpt::module::command_note);
                        printf("%s", t.c_str());
                        t = mod.format_pattern_row_channel_command(p, r, c, openmpt::module::command_effect);
                        printf("%s", t.c_str());
                        t = mod.format_pattern_row_channel_command(p, r, c, openmpt::module::command_parameter);
                        printf("%s ", t.c_str());
                    }
                }

                if (dump_patterns)
                    printf("\n");
            }

            if (dump_patterns)
                fflush(stdout);
        }
    }

    return hash;
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

extern "C"
{
    // Need to match in Rust
    struct CSampleData {
        // sample data
        uint8_t* data;
        // Name/text of the sample 
        uint8_t* sample_text;
        // length in bytes
        uint32_t length_bytes;
        // length in bytes
        uint32_t length;
        // Id for the samele in the song 
        uint32_t sample_id;
        // Global volume (sample volume is multiplied by this), 0...64
        uint16_t global_vol;
        // bits per sample
        uint8_t bits_per_sample;
        // if stero sample or not
        uint8_t stereo;
        // Default sample panning (if pan flag is set), 0...256
        uint16_t pan;
        // Default volume, 0...256 (ignored if uFlags[SMP_NODEFAULTVOLUME] is set)
        uint16_t volume;
        // Frequency of middle-C, in Hz (for IT/S3M/MPTM)
        uint32_t c5_speed;
        // Relative note to middle c (for MOD/XM)
        int8_t relative_tone;
        // Finetune period (for MOD/XM), -128...127, unit is 1/128th of a semitone
        int8_t fine_tune;
        // Auto vibrato type
        uint8_t vib_type;
        // Auto vibrato sweep (i.e. how long it takes until the vibrato effect reaches its full depth)
        uint8_t vib_sweep;
        // Auto vibrato depth
        uint8_t vib_depth;
        // Auto vibrato rate (speed)
        uint8_t vib_rate;
    };

    struct CData {
        uint64_t hash;
        CSampleData* samples;
        char** instrument_names;
        int sample_count;
        int instrument_count;
        int channel_count;
        // must be last
        openmpt::module* mod;
    };

    CData* hash_file(unsigned char* buffer, int len, int dump_patterns) {
        /*
        if (g_debug_heap == nullptr) {
            g_debug_heap = DebugHeapInit(1024 * 1024 * 1024);
        }
        */

        // uint64_t hash = 0;
        CData* data = nullptr;

        try {
            data = (CData*)DebugHeapAllocate(g_debug_heap, sizeof(CData), alignof(CData)); 
            memset(data, 0, sizeof(CData));

            openmpt::detail::initial_ctls_map ctls;
            //ctls["load.skip_samples"] = "1";
            ctls["load.skip_plugins"] = "1";
            data->mod = new openmpt::module(buffer, (size_t)len, std::clog, ctls);
            
            OpenMPT::CSoundFile* sf = data->mod->get_snd_file();
            
            int samples_count = sf->GetNumSamples();
            int instrument_count = sf->GetNumInstruments();

            CSampleData* samples = nullptr;

            if (samples_count > 0) {
                samples = (CSampleData*)DebugHeapAllocate(g_debug_heap, sizeof(CSampleData) * samples_count, alignof(CSampleData));

                for (int i = 1; i < samples_count + 1; i++) {
                    CSampleData& sample = samples[i - 1]; 
                    const auto& mod_sample = sf->GetSample(i);

                    sample.data = (uint8_t*)mod_sample.pData.pSample8;
                    sample.sample_text = (uint8_t*)sf->GetSampleName(i);
                    sample.length_bytes = mod_sample.GetSampleSizeInBytes(); 
                    sample.length = mod_sample.nLength;
                    sample.sample_id = i;
                    sample.global_vol = mod_sample.nGlobalVol; 
                    sample.bits_per_sample = mod_sample.uFlags[OpenMPT::CHN_16BIT] ? 16 : 8;
                    sample.stereo = mod_sample.uFlags[OpenMPT::CHN_STEREO] ? 1 : 0;
                    sample.pan = mod_sample.nPan;
                    sample.volume = mod_sample.nVolume;
                    sample.c5_speed = mod_sample.nC5Speed;
                    sample.relative_tone = mod_sample.RelativeTone;
                    sample.fine_tune = mod_sample.nFineTune;
                    sample.vib_type = mod_sample.nVibType;
                    sample.vib_sweep = mod_sample.nVibSweep;
                    sample.vib_depth = mod_sample.nVibDepth;
                    sample.vib_rate = mod_sample.nVibRate;
                }
            }

            char** instruments = nullptr;

            if (instrument_count > 0) {
                instruments = (char**)DebugHeapAllocate(g_debug_heap, sizeof(char*) * instrument_count, alignof(char*)); 

                for (int i = 0; i < instrument_count; i++) {
                    instruments[i] = (char*)sf->GetInstrumentName(i);
                }
            } 

            data->hash = hash_patterns(*data->mod, dump_patterns);
            data->samples = samples;
            data->sample_count = samples_count;
            data->instrument_names = instruments;
            data->instrument_count = instrument_count;
            data->channel_count = data->mod->get_num_channels();
        }
        catch (const std::exception &e) {
            // std::cout << "Cannot open " << filename << ": " << (e.what() ? e.what() : "unknown error") << std::endl;
        }

        return data;
    }

    void free_hash_data(CData* data) {
        if (data == nullptr) {
            return;
        }

        if (data->mod) {
            delete data->mod;
        }

        if (data->samples)
            DebugHeapFree(g_debug_heap, data->samples);

        if (data->instrument_names)
            DebugHeapFree(g_debug_heap, data->instrument_names);

        DebugHeapFree(g_debug_heap, data);
    }
}
