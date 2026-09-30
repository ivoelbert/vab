"""Validate the Snow Bros. alternative and its two-button control layout."""
import unittest
import test_build_mk2 as common
import build_snowbros as cabinet


class SnowBrosTests(common.CabinetTests):
    @classmethod
    def setUpClass(cls):
        tex, solids = cabinet.textures(), cabinet.model()
        cls.views = [cabinet.base.render(solids, tex, turns, painter=cabinet.paint)
                     for turns in range(4)]

    def test_two_buttons_and_one_cyan_stick_per_player(self):
        solids = cabinet.model()
        self.assertEqual(sum(isinstance(solid, cabinet.base.Ball) for solid in solids), 2)
        self.assertEqual(sum(solid.part == 'stick' for solid in solids), 2)
        buttons = [solid for solid in solids if solid.part == 'button'
                   and isinstance(solid, cabinet.base.Solid)]
        self.assertEqual(len(buttons), 4)
        self.assertEqual({solid.color for solid in buttons}, {(44, 127, 232)})

    def test_opposite_sides_keep_their_distinct_decals(self):
        tex = cabinet.textures()
        self.assertNotEqual(tex['left'].tobytes(), tex['right'].tobytes())
        self.assertEqual(tex['left'].size, tex['right'].size)


if __name__ == '__main__':
    unittest.main()
