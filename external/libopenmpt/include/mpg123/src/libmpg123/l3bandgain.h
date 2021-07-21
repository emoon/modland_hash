/*
	l3bandinfo.h: the layer III bandInfo structures and gainpow2_scale

	copyright 1995-2021 by the mpg123 project - free software under the terms of the LGPL 2.1
	see COPYING and AUTHORS files in distribution or http://mpg123.org
	initially written by Michael Hipp

	This is separated out of layer3.c to re-use independently in calctables.
*/

struct bandInfoStruct
{
	unsigned short longIdx[23];
	unsigned char longDiff[22];
	unsigned short shortIdx[14];
	unsigned char shortDiff[13];
};

/* Techy details about our friendly MPEG data. Fairly constant over the years;-) */
static const struct bandInfoStruct bandInfo[9] =
{
	{ /* MPEG 1.0 */
		{0,4,8,12,16,20,24,30,36,44,52,62,74, 90,110,134,162,196,238,288,342,418,576},
		{4,4,4,4,4,4,6,6,8, 8,10,12,16,20,24,28,34,42,50,54, 76,158},
		{0,4*3,8*3,12*3,16*3,22*3,30*3,40*3,52*3,66*3, 84*3,106*3,136*3,192*3},
		{4,4,4,4,6,8,10,12,14,18,22,30,56}
	},
	{
		{0,4,8,12,16,20,24,30,36,42,50,60,72, 88,106,128,156,190,230,276,330,384,576},
		{4,4,4,4,4,4,6,6,6, 8,10,12,16,18,22,28,34,40,46,54, 54,192},
		{0,4*3,8*3,12*3,16*3,22*3,28*3,38*3,50*3,64*3, 80*3,100*3,126*3,192*3},
		{4,4,4,4,6,6,10,12,14,16,20,26,66}
	},
	{
		{0,4,8,12,16,20,24,30,36,44,54,66,82,102,126,156,194,240,296,364,448,550,576},
		{4,4,4,4,4,4,6,6,8,10,12,16,20,24,30,38,46,56,68,84,102, 26},
		{0,4*3,8*3,12*3,16*3,22*3,30*3,42*3,58*3,78*3,104*3,138*3,180*3,192*3},
		{4,4,4,4,6,8,12,16,20,26,34,42,12}
	},
	{ /* MPEG 2.0 */
		{0,6,12,18,24,30,36,44,54,66,80,96,116,140,168,200,238,284,336,396,464,522,576},
		{6,6,6,6,6,6,8,10,12,14,16,20,24,28,32,38,46,52,60,68,58,54 } ,
		{0,4*3,8*3,12*3,18*3,24*3,32*3,42*3,56*3,74*3,100*3,132*3,174*3,192*3} ,
		{4,4,4,6,6,8,10,14,18,26,32,42,18 }
	},
	{ /* Twiddling 3 values here (not just 330->332!) fixed bug 1895025. */
		{0,6,12,18,24,30,36,44,54,66,80,96,114,136,162,194,232,278,332,394,464,540,576},
		{6,6,6,6,6,6,8,10,12,14,16,18,22,26,32,38,46,54,62,70,76,36 },
		{0,4*3,8*3,12*3,18*3,26*3,36*3,48*3,62*3,80*3,104*3,136*3,180*3,192*3},
		{4,4,4,6,8,10,12,14,18,24,32,44,12 }
	},
	{
		{0,6,12,18,24,30,36,44,54,66,80,96,116,140,168,200,238,284,336,396,464,522,576},
		{6,6,6,6,6,6,8,10,12,14,16,20,24,28,32,38,46,52,60,68,58,54 },
		{0,4*3,8*3,12*3,18*3,26*3,36*3,48*3,62*3,80*3,104*3,134*3,174*3,192*3},
		{4,4,4,6,8,10,12,14,18,24,30,40,18 }
	},
	{ /* MPEG 2.5 */
		{0,6,12,18,24,30,36,44,54,66,80,96,116,140,168,200,238,284,336,396,464,522,576},
		{6,6,6,6,6,6,8,10,12,14,16,20,24,28,32,38,46,52,60,68,58,54},
		{0,12,24,36,54,78,108,144,186,240,312,402,522,576},
		{4,4,4,6,8,10,12,14,18,24,30,40,18}
	},
	{
		{0,6,12,18,24,30,36,44,54,66,80,96,116,140,168,200,238,284,336,396,464,522,576},
		{6,6,6,6,6,6,8,10,12,14,16,20,24,28,32,38,46,52,60,68,58,54},
		{0,12,24,36,54,78,108,144,186,240,312,402,522,576},
		{4,4,4,6,8,10,12,14,18,24,30,40,18}
	},
	{
		{0,12,24,36,48,60,72,88,108,132,160,192,232,280,336,400,476,566,568,570,572,574,576},
		{12,12,12,12,12,12,16,20,24,28,32,40,48,56,64,76,90,2,2,2,2,2},
		{0, 24, 48, 72,108,156,216,288,372,480,486,492,498,576},
		{8,8,8,12,16,20,24,28,36,2,2,2,26}
	}
};

#if defined(REAL_IS_FIXED) || defined(FORCE_FIXED)
static const char gainpow2_scale[256+118+4+1] = 
{
	19,19,19,20,20,20,20,21,21,21,21,22,22,22,22,23,23,23,23,24,24,24,24,25,25,25,25,26,26,26,26,27,
	27,27,27,28,28,28,28,29,29,29,29,30,30,30,30,31,31,31,31,32,32,32,32,33,33,33,33,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,
	34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,0
};
#endif
