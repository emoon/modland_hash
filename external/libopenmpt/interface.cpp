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

#ifndef _WIN32
#include <sys/prctl.h>
#include <linux/seccomp.h>
#endif

#include <libopenmpt/libopenmpt.hpp>

static uint64_t hash_patterns(openmpt::module &mod)
{
    // Hash pattern data using 64-bit FNV-1a
    uint64_t hash = 14695981039346656037ull;

    const auto num_channels = mod.get_num_channels();
    const auto num_songs = mod.get_num_subsongs();
    for (auto s = 0; s < num_songs; s++)
    {
        mod.select_subsong(s);
        if (mod.get_current_order() != 0)
        {
            // Ignore hidden subsongs, as we go through the whole oder list anyway.
            continue;
        }

        const auto num_orders = mod.get_num_orders();
        // Go through the complete sequence order by order.
        for (auto o = 0; o < num_orders; o++)
        {
            const auto p = mod.get_order_pattern(o);
            const auto num_rows = mod.get_pattern_num_rows(p);
            for (auto c = 0; c < num_channels; c++)
            {
                for (auto r = 0; r < num_rows; r++)
                {
                    const uint8_t note = mod.get_pattern_row_channel_command(p, r, c, openmpt::module::command_note);
                    if (note != 0)
                    {
                        hash ^= note;
                        hash *= 1099511628211ull;
                    }
                }
            }
        }
    }
    return hash;
}

extern "C"
{

    struct CData
    {
        uint64_t hash;
        char *sample_names;
        char *artist;
        char *comments;
        int channel_count;
    };

    CData *hash_file(const char *filename)
    {
        // uint64_t hash = 0;
        CData *data = nullptr;

        try
        {
            std::stringstream file;
            {
                std::ifstream is(filename, std::ios::binary);
                if (!is.good())
                {
                    // std::cout << "Cannot open " << filename << " for reading" << std::endl;
                    return 0;
                }

                file << is.rdbuf();
            }

            openmpt::detail::initial_ctls_map ctls;
            ctls["load.skip_samples"] = "1";
            ctls["load.skip_plugins"] = "1";
            openmpt::module mod(file, std::clog, ctls);

            std::string extension = mod.get_metadata("type");
            std::string artist = mod.get_metadata("artist");
            std::string name = mod.get_metadata("title");
            std::string instruments;

            const auto &instrument_names = mod.get_instrument_names();
            const auto &sample_names = mod.get_sample_names();
            // int skip_from_line = 0;

            for (const auto &t : instrument_names)
            {
                instruments += t + "\n";
            }

            for (const auto &t : sample_names)
            {
                instruments += t + "\n";
            }

            /*
            for (int i = 0; i < instrument_names.size(); ++i) {
                if (!(instrument_names[i] == "" || instrument_names[i] == " ")) {
                    skip_from_line = i;
                }
            }

            for (int i = 0; i < skip_from_line; ++i) {
                instruments += instrument_names[i] + "\n";
            }

            skip_from_line = 0;

            for (int i = 0; i < sample_names.size(); ++i) {
                if (!(sample_names[i] == "" || sample_names[i] == " ")) {
                    skip_from_line = i;
                }
            }

            for (int i = 0; i < skip_from_line; ++i) {
                instruments += sample_names[i] + "\n";
            }
            */

            std::string comments = mod.get_metadata("message_raw");

            // auto pattern_hash = hash_patterns(mod);
            // hash = hash_patterns(mod);

            data = new CData;
            data->hash = hash_patterns(mod);
            data->sample_names = strdup(instruments.c_str());
            data->artist = strdup(artist.c_str());
            data->comments = strdup(comments.c_str());
            data->channel_count = mod.get_num_channels();
        }
        catch (const std::exception &e)
        {
            // std::cout << "Cannot open " << filename << ": " << (e.what() ? e.what() : "unknown error") << std::endl;
        }

        return data;
    }

    void free_hash_data(CData *data)
    {
        free(data->sample_names);
        free(data->artist);
        free(data->comments);
        delete data;
    }
}
